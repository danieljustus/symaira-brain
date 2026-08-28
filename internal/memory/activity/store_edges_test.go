package activity

import (
	"testing"
	"time"
)

func TestActivityStoreRemainingBehaviorEdges(t *testing.T) {
	base := time.Date(2026, 8, 28, 12, 0, 0, 0, time.UTC)
	store, database, _ := newActivityStore(t, Options{Now: func() time.Time { return base }})
	segment := Segment{ID: "edge-segment", Source: "source", Granularity: Granularity10Min,
		StartedAt: base, EndedAt: base.Add(time.Minute), Applications: []string{"Editor"},
		RedactedSummary: "summary"}
	if err := store.SaveSegment(segment); err != nil {
		t.Fatal(err)
	}
	if _, err := store.RollUp6Hour("other-source", base); err == nil {
		t.Fatal("empty rollup unexpectedly succeeded")
	}
	rollup, err := store.RollUp6Hour("source", base)
	if err != nil {
		t.Fatal(err)
	}
	all, err := store.ListAllSegments(base.Add(-time.Minute), base.Add(time.Hour))
	if err != nil || len(all) != 2 {
		t.Fatalf("all segments = %#v, err=%v", all, err)
	}
	if err := store.SaveEpisode(Episode{Title: "generated", Scope: "scope", StartedAt: base,
		EndedAt: base.Add(time.Hour), Confidence: 0.5, SegmentIDs: []string{rollup.ID}}); err != nil {
		t.Fatal(err)
	}
	generated, err := store.ListEpisodes(base.Add(-time.Minute), base.Add(2*time.Hour))
	if err != nil || len(generated) != 1 || generated[0].ID == "" {
		t.Fatalf("generated episode = %#v, err=%v", generated, err)
	}
	if _, err := store.StageEpisode("missing-episode"); err == nil {
		t.Fatal("missing episode unexpectedly staged")
	}
	if err := store.SaveEpisode(Episode{ID: "missing-source-episode", Title: "missing source", StartedAt: base,
		EndedAt: base.Add(time.Hour), Confidence: 0.5, SegmentIDs: []string{"missing-segment"}}); err != nil {
		t.Fatal(err)
	}
	if _, err := store.StageEpisode("missing-source-episode"); err == nil {
		t.Fatal("episode with missing source unexpectedly staged")
	}
	if _, err := database.Conn().Exec("UPDATE activity_segments SET applications = ? WHERE id = ?", "not-json", segment.ID); err != nil {
		t.Fatal(err)
	}
	if _, err := store.GetSegment(segment.ID); err == nil {
		t.Fatal("invalid segment JSON unexpectedly decoded")
	}
	if _, err := database.Conn().Exec("UPDATE activity_episodes SET sources = ? WHERE id = ?", "not-json", generated[0].ID); err != nil {
		t.Fatal(err)
	}
	if _, err := store.GetEpisode(generated[0].ID); err == nil {
		t.Fatal("invalid episode JSON unexpectedly decoded")
	}
}
