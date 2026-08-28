package activity

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

func newActivityStore(t *testing.T, opts Options) (*Store, *db.DB, string) {
	t.Helper()
	path := filepath.Join(t.TempDir(), "activity.db")
	cfg := config.Defaults()
	cfg.Database.Path = path
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatalf("open db: %v", err)
	}
	t.Cleanup(func() { _ = database.Close() })
	store, err := NewStore(database, opts)
	if err != nil {
		t.Fatalf("new activity store: %v", err)
	}
	return store, database, path
}

func fixtureSegments(t *testing.T) []Segment {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", "segments.json"))
	if err != nil {
		t.Fatal(err)
	}
	var raw []struct {
		Source          string    `json:"source"`
		Granularity     string    `json:"granularity"`
		RedactedSummary string    `json:"redacted_summary"`
		RawRef          string    `json:"raw_ref"`
		StartedAt       time.Time `json:"started_at"`
		EndedAt         time.Time `json:"ended_at"`
		Applications    []string  `json:"applications"`
		PriorSegmentIDs []string  `json:"prior_segment_ids"`
	}
	if err := json.Unmarshal(data, &raw); err != nil {
		t.Fatal(err)
	}
	segments := make([]Segment, len(raw))
	for i, value := range raw {
		segments[i] = Segment{
			Source: value.Source, Granularity: value.Granularity,
			StartedAt: value.StartedAt, EndedAt: value.EndedAt,
			Applications: value.Applications, RedactedSummary: value.RedactedSummary,
			RawRef: value.RawRef, PriorSegmentIDs: value.PriorSegmentIDs,
		}
	}
	return segments
}

func TestActivitySchemaAndPIIRedactionBeforePersistence(t *testing.T) {
	now := time.Date(2026, 8, 28, 12, 0, 0, 0, time.UTC)
	store, database, _ := newActivityStore(t, Options{Now: func() time.Time { return now }})
	segment := Segment{
		Source: "codex-skysight", Granularity: Granularity10Min,
		StartedAt: now, EndedAt: now.Add(10 * time.Minute),
		Applications:    []string{"com.example.editor"},
		RedactedSummary: "Contact synthetic@example.invalid about the activity.",
		RawRef:          "https://user:password@example.invalid/source/001",
	}
	if err := store.SaveSegment(segment); err != nil {
		t.Fatalf("save segment: %v", err)
	}
	got, err := store.ListAllSegments(now.Add(-time.Minute), now.Add(time.Hour))
	if err != nil || len(got) != 1 {
		t.Fatalf("segments = %#v, err=%v", got, err)
	}
	if strings.Contains(got[0].RedactedSummary, "synthetic@example.invalid") ||
		strings.Contains(got[0].RawRef, "user:password") {
		t.Fatalf("PII reached activity storage: %+v", got[0])
	}
	if got[0].RawRef == "" || !strings.Contains(got[0].RawRef, "REDACTED") {
		t.Fatalf("redacted raw reference = %q", got[0].RawRef)
	}
	for _, table := range []string{"activity_segments", "activity_episodes"} {
		var count int
		if err := database.Conn().QueryRow("SELECT COUNT(*) FROM " + table).Scan(&count); err != nil {
			t.Fatalf("schema table %s missing: %v", table, err)
		}
	}
	var columns string
	if err := database.Conn().QueryRow("SELECT sql FROM sqlite_master WHERE name = 'activity_segments'").Scan(&columns); err != nil {
		t.Fatal(err)
	}
	if strings.Contains(strings.ToLower(columns), "raw_payload") {
		t.Fatal("activity schema must not persist raw payloads")
	}
}

func TestActivityRetentionDefaultsTo48HoursAndCanAdvanceClock(t *testing.T) {
	base := time.Date(2026, 8, 28, 12, 0, 0, 0, time.UTC)
	clock := base
	store, database, path := newActivityStore(t, Options{Now: func() time.Time { return clock }})
	if err := store.SaveSegment(Segment{ID: "retained", Source: "symcockpit", Granularity: Granularity10Min,
		StartedAt: base, EndedAt: base.Add(10 * time.Minute), RedactedSummary: "synthetic retained observation"}); err != nil {
		t.Fatal(err)
	}
	var expiry time.Time
	if err := database.Conn().QueryRow("SELECT expires_at FROM activity_segments WHERE id = ?", "retained").Scan(&expiry); err != nil {
		t.Fatal(err)
	}
	if !expiry.Equal(base.Add(10*time.Minute + 48*time.Hour)) {
		t.Fatalf("expiry = %v, want %v", expiry, base.Add(10*time.Minute+48*time.Hour))
	}
	clock = base.Add(48*time.Hour + 10*time.Minute)
	result, err := store.Expire()
	if err != nil {
		t.Fatalf("expire: %v", err)
	}
	if result.Segments != 1 {
		t.Fatalf("expired segments = %d, want 1", result.Segments)
	}
	var count int
	if err := database.Conn().QueryRow("SELECT COUNT(*) FROM activity_segments").Scan(&count); err != nil || count != 0 {
		t.Fatalf("segment count after expiry = %d, err=%v", count, err)
	}
	if err := store.VerifyDiskClean(); err != nil {
		t.Fatalf("disk cleanup: %v", err)
	}
	if info, err := os.Stat(path + "-wal"); err == nil && info.Size() != 0 {
		t.Fatalf("WAL still contains %d bytes after expiry", info.Size())
	}
}

func TestActivitySixHourRollupIsIdempotentAndTakesPrecedence(t *testing.T) {
	base := time.Date(2026, 8, 28, 10, 0, 0, 0, time.UTC)
	store, database, _ := newActivityStore(t, Options{Now: func() time.Time { return base }})
	for _, segment := range fixtureSegments(t) {
		if err := store.SaveSegment(segment); err != nil {
			t.Fatal(err)
		}
	}
	rollup, err := store.RollUp6Hour("codex-skysight", base)
	if err != nil {
		t.Fatalf("rollup: %v", err)
	}
	if len(rollup.PriorSegmentIDs) != 3 || rollup.PriorSegmentIDs[0] == "" {
		t.Fatalf("prior references = %#v, want fine segments and prior reference", rollup.PriorSegmentIDs)
	}
	visible, err := store.ListSegments(base.Add(-time.Minute), base.Add(7*time.Hour))
	if err != nil || len(visible) != 1 || visible[0].Granularity != Granularity6Hour {
		t.Fatalf("visible segments = %#v, err=%v", visible, err)
	}
	all, err := store.ListAllSegments(base.Add(-time.Minute), base.Add(7*time.Hour))
	if err != nil || len(all) != 3 {
		t.Fatalf("all segments = %d, err=%v, want 3", len(all), err)
	}
	if _, err := store.RollUp6Hour("codex-skysight", base); err != nil {
		t.Fatalf("rerun rollup: %v", err)
	}
	var count int
	if err := database.Conn().QueryRow("SELECT COUNT(*) FROM activity_segments WHERE granularity = '6h'").Scan(&count); err != nil || count != 1 {
		t.Fatalf("6h rollup count = %d, err=%v", count, err)
	}
	all, err = store.ListAllSegments(base.Add(-time.Minute), base.Add(7*time.Hour))
	if err != nil || len(all) != 3 {
		t.Fatalf("all after rerun = %d, err=%v", len(all), err)
	}
}

func TestActivityDeletionByEpisodeRangeAndAllIsVerified(t *testing.T) {
	base := time.Date(2026, 8, 28, 8, 0, 0, 0, time.UTC)
	store, database, _ := newActivityStore(t, Options{Now: func() time.Time { return base }})
	segments := []Segment{
		{ID: "delete-a", Source: "symcockpit", Granularity: Granularity10Min, StartedAt: base, EndedAt: base.Add(10 * time.Minute), RedactedSummary: "synthetic delete A"},
		{ID: "delete-b", Source: "symcockpit", Granularity: Granularity10Min, StartedAt: base.Add(time.Hour), EndedAt: base.Add(time.Hour + 10*time.Minute), RedactedSummary: "synthetic delete B"},
	}
	for _, segment := range segments {
		if err := store.SaveSegment(segment); err != nil {
			t.Fatal(err)
		}
	}
	if err := store.SaveEpisode(Episode{ID: "episode-a", Title: "Synthetic episode A", Scope: "project-a",
		StartedAt: base, EndedAt: base.Add(10 * time.Minute), Confidence: 0.8, SegmentIDs: []string{"delete-a"}}); err != nil {
		t.Fatal(err)
	}
	result, err := store.DeleteEpisode("episode-a")
	if err != nil || result.Episodes != 1 || result.Segments != 1 {
		t.Fatalf("delete episode = %+v, err=%v", result, err)
	}
	if _, err := store.ClearTimeRange(base.Add(50*time.Minute), base.Add(2*time.Hour)); err != nil {
		t.Fatalf("clear range: %v", err)
	}
	if result, err := store.ClearAll(); err != nil {
		t.Fatalf("clear all: %v", err)
	} else if result.Segments != 0 || result.Episodes != 0 {
		t.Fatalf("clear all after range = %+v, want empty", result)
	}
	for _, table := range []string{"activity_segments", "activity_episodes"} {
		var count int
		if err := database.Conn().QueryRow("SELECT COUNT(*) FROM " + table).Scan(&count); err != nil || count != 0 {
			t.Fatalf("%s count = %d, err=%v", table, count, err)
		}
	}
	if err := store.VerifyDiskClean(); err != nil {
		t.Fatal(err)
	}
}

func TestActivityPromotionStagesMemoryWithEvidenceAndExcludesSync(t *testing.T) {
	base := time.Date(2026, 8, 28, 8, 0, 0, 0, time.UTC)
	store, database, _ := newActivityStore(t, Options{Now: func() time.Time { return base }})
	if err := store.SaveSegment(Segment{ID: "promote-a", Source: "symcockpit", Granularity: Granularity6Hour,
		StartedAt: base, EndedAt: base.Add(6 * time.Hour), RedactedSummary: "Synthetic recurring project activity"}); err != nil {
		t.Fatal(err)
	}
	if err := store.SaveEpisode(Episode{ID: "episode-promote", Title: "Synthetic recurring work", Scope: "project-a",
		StartedAt: base, EndedAt: base.Add(6 * time.Hour), Confidence: 0.75, SegmentIDs: []string{"promote-a"}}); err != nil {
		t.Fatal(err)
	}
	candidate, err := store.StageEpisode("episode-promote")
	if err != nil {
		t.Fatalf("stage episode: %v", err)
	}
	if candidate.ReviewStatus != db.ReviewStaged || candidate.Metadata["sync_exclude"] != "true" {
		t.Fatalf("candidate = %+v, want staged and sync-excluded", candidate)
	}
	var oplogCount int
	if err := database.Conn().QueryRow("SELECT COUNT(*) FROM sync_oplog WHERE memory_id = ?", candidate.ID).Scan(&oplogCount); err != nil || oplogCount != 0 {
		t.Fatalf("activity candidate oplog count = %d, err=%v", oplogCount, err)
	}
	evidence, err := database.GetMemoryEvidence(candidate.ID)
	if err != nil || len(evidence) != 1 || evidence[0].SourceID != "promote-a" {
		t.Fatalf("candidate evidence = %#v, err=%v", evidence, err)
	}
	if err := database.SetMemoryReviewStatus(candidate.ID, db.ReviewApproved); err != nil {
		t.Fatal(err)
	}
	if err := database.DeleteMemory(candidate.ID); err != nil {
		t.Fatal(err)
	}
	if err := database.Conn().QueryRow("SELECT COUNT(*) FROM sync_oplog WHERE memory_id = ?", candidate.ID).Scan(&oplogCount); err != nil || oplogCount != 0 {
		t.Fatalf("promoted activity candidate delete entered oplog: %d, err=%v", oplogCount, err)
	}
}

func TestActivitySingleFineWindowCannotBePromoted(t *testing.T) {
	base := time.Date(2026, 8, 28, 8, 0, 0, 0, time.UTC)
	store, _, _ := newActivityStore(t, Options{Now: func() time.Time { return base }})
	if err := store.SaveSegment(Segment{ID: "fine-only", Source: "symcockpit", Granularity: Granularity10Min,
		StartedAt: base, EndedAt: base.Add(10 * time.Minute), RedactedSummary: "one synthetic observation"}); err != nil {
		t.Fatal(err)
	}
	if err := store.SaveEpisode(Episode{ID: "fine-episode", Title: "One window", Scope: "project-a",
		StartedAt: base, EndedAt: base.Add(10 * time.Minute), Confidence: 0.8, SegmentIDs: []string{"fine-only"}}); err != nil {
		t.Fatal(err)
	}
	if _, err := store.StageEpisode("fine-episode"); err == nil {
		t.Fatal("single 10min observation should not cross promotion boundary")
	}
}
