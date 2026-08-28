package activity

import (
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

func TestActivityEpisodeReadAndStoreAliases(t *testing.T) {
	base := time.Date(2026, 8, 28, 8, 0, 0, 0, time.UTC)
	store, database, _ := newActivityStore(t, Options{Now: func() time.Time { return base }})
	segments := []Segment{
		{ID: "alias-a", Source: "symcockpit", Granularity: Granularity10Min, StartedAt: base, EndedAt: base.Add(10 * time.Minute), RedactedSummary: "edited code"},
		{ID: "alias-b", Source: "symcockpit", Granularity: Granularity10Min, StartedAt: base.Add(10 * time.Minute), EndedAt: base.Add(20 * time.Minute), RedactedSummary: "ran tests"},
	}
	for _, segment := range segments {
		if err := store.PutSegment(segment); err != nil {
			t.Fatal(err)
		}
	}
	rollup, err := store.Rollup("symcockpit", base)
	if err != nil {
		t.Fatalf("rollup alias: %v", err)
	}
	if err := JSONShapeCheck(*rollup); err != nil {
		t.Fatalf("JSON shape: %v", err)
	}
	if store.Database() != database || !store.DatabaseFileExists() {
		t.Fatal("store aliases do not expose the shared database")
	}

	episode := Episode{
		ID: "alias-episode", Title: "Project work", Scope: "project-a",
		StartedAt: base, EndedAt: base.Add(6 * time.Hour), Confidence: 0.8,
		SegmentIDs: []string{rollup.ID}, Citations: []string{"https://example.invalid/activity"},
	}
	if err := store.PutEpisode(episode); err != nil {
		t.Fatalf("put episode: %v", err)
	}
	got, err := store.GetEpisode(episode.ID)
	if err != nil || got == nil || got.Title != episode.Title {
		t.Fatalf("get episode = %#v, err=%v", got, err)
	}
	listed, err := store.ListEpisodes(base.Add(-time.Minute), base.Add(7*time.Hour))
	if err != nil || len(listed) != 1 || listed[0].ID != episode.ID {
		t.Fatalf("listed episodes = %#v, err=%v", listed, err)
	}
	page, err := store.Search(SearchOptions{
		Query: "project", From: base.Add(-time.Minute), To: base.Add(7 * time.Hour),
		Limit: 10, MaxTokens: 100, IncludeEpisodes: true,
	})
	if err != nil || len(page.Results) != 1 || page.Results[0].Kind != "episode" {
		t.Fatalf("episode search = %#v, err=%v", page, err)
	}
	item, err := store.GetReadItem(episode.ID)
	if err != nil || item == nil || item.Kind != "episode" || len(item.Provenance.DerivedFrom) != 1 {
		t.Fatalf("episode read item = %#v, err=%v", item, err)
	}
	status, err := store.Status()
	if err != nil || status.ActiveSegments != 3 || status.ActiveEpisodes != 1 || status.Earliest == nil || status.Latest == nil {
		t.Fatalf("status = %#v, err=%v", status, err)
	}

	candidate, err := store.PromoteEpisodeToCandidate(episode.ID)
	if err != nil || candidate == nil {
		t.Fatalf("promote-to-candidate = %#v, err=%v", candidate, err)
	}
	aliasCandidate, err := store.PromoteEpisode(episode.ID)
	if err != nil || aliasCandidate == nil || aliasCandidate.ID != candidate.ID {
		t.Fatalf("promote alias = %#v, err=%v", aliasCandidate, err)
	}

	result, err := store.ClearLastSession()
	if err != nil || result.Episodes != 1 || result.Segments != 1 {
		t.Fatalf("clear last session = %+v, err=%v", result, err)
	}
	if result, err := store.PurgeExpired(base.Add(24 * time.Hour)); err != nil || result != (RetentionResult{}) {
		t.Fatalf("purge expired no-op = %+v, err=%v", result, err)
	}
	if result, err := store.ClearEpisode(episode.ID); err != nil || result != (RetentionResult{}) {
		t.Fatalf("clear missing episode = %+v, err=%v", result, err)
	}
	if result, err := store.ClearAllActivity(); err != nil || result.Episodes != 0 || result.Segments != 2 {
		t.Fatalf("clear all alias = %+v, err=%v", result, err)
	}
}

func TestActivityOptionsAndValidationDefaults(t *testing.T) {
	if DefaultSegmentTTL() != 48*time.Hour || DefaultEpisodeTTL() != 30*24*time.Hour {
		t.Fatal("unexpected activity retention defaults")
	}
	configured := NewOptionsFromConfig("2h", "72h")
	if configured.SegmentTTL != 2*time.Hour || configured.EpisodeTTL != 72*time.Hour {
		t.Fatalf("configured options = %+v", configured)
	}
	fallback := NewOptionsFromConfig("invalid", "-1h")
	if fallback.SegmentTTL != DefaultSegmentTTL() || fallback.EpisodeTTL != DefaultEpisodeTTL() {
		t.Fatalf("fallback options = %+v", fallback)
	}
	store, database, _ := newActivityStore(t, Options{})
	cfg := config.Defaults()
	cfg.Retention.ActivitySegmentTTL = "1h"
	cfg.Retention.ActivityEpisodeTTL = "2h"
	fromConfig, err := NewStoreFromConfig(database, cfg)
	if err != nil || fromConfig == nil {
		t.Fatalf("new store from config = %#v, err=%v", fromConfig, err)
	}
	if _, err := NewStore(nil); err == nil {
		t.Fatal("nil database unexpectedly accepted")
	}
	if _, err := NewStoreFromConfig(nil, cfg); err == nil {
		t.Fatal("nil database from config unexpectedly accepted")
	}
	if store.DatabaseFileExists() == false {
		t.Fatal("new store database file missing")
	}
}
