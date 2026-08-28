package activity

import (
	"testing"
	"time"
)

func TestActivityClosedDatabaseErrorPaths(t *testing.T) {
	store, database, _ := newActivityStore(t, Options{})
	base := store.now()
	segment := Segment{ID: "closed-segment", Source: "source", Granularity: Granularity10Min,
		StartedAt: base, EndedAt: base.Add(time.Minute), RedactedSummary: "summary"}
	episode := Episode{ID: "closed-episode", Title: "episode", StartedAt: base,
		EndedAt: base.Add(time.Hour), Confidence: 0.5, SegmentIDs: []string{"closed-segment"}}

	if _, err := NewStoreFromConfig(database, nil); err != nil {
		t.Fatalf("new store with default config: %v", err)
	}
	if err := store.SaveSegment(segment); err != nil {
		t.Fatalf("seed segment: %v", err)
	}
	if err := store.SaveEpisode(episode); err != nil {
		t.Fatalf("seed episode: %v", err)
	}
	segmentRows, err := database.Conn().Query(`SELECT id, source, granularity, started_at, ended_at,
		applications, redacted_summary, raw_ref, prior_segment_ids, superseded_by, expires_at
		FROM activity_segments WHERE id = ?`, segment.ID)
	if err != nil {
		t.Fatal(err)
	}
	if !segmentRows.Next() {
		t.Fatal("segment row missing")
	}
	if _, err := scanSegmentRows(segmentRows); err != nil {
		t.Fatalf("scan segment rows: %v", err)
	}
	if err := segmentRows.Close(); err != nil {
		t.Fatal(err)
	}
	episodeRows, err := database.Conn().Query(`SELECT id, title, scope, started_at, ended_at,
		confidence, sources, citations, expires_at FROM activity_episodes WHERE id = ?`, episode.ID)
	if err != nil {
		t.Fatal(err)
	}
	if !episodeRows.Next() {
		t.Fatal("episode row missing")
	}
	if _, err := scanEpisodeRows(episodeRows); err != nil {
		t.Fatalf("scan episode rows: %v", err)
	}
	if err := episodeRows.Close(); err != nil {
		t.Fatal(err)
	}

	if err := database.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err := NewStore(database); err == nil {
		t.Fatal("new store on closed DB unexpectedly succeeded")
	}
	search := SearchOptions{Query: "query", From: base, To: base.Add(time.Hour), Limit: 1, MaxTokens: 10}
	if err := store.SaveSegment(Segment{}); err == nil {
		t.Fatal("invalid segment unexpectedly accepted")
	}
	if err := store.SaveEpisode(Episode{}); err == nil {
		t.Fatal("invalid episode unexpectedly accepted")
	}
	if err := store.SaveSegment(segment); err == nil {
		t.Fatal("save segment on closed DB unexpectedly succeeded")
	}
	if _, err := store.RollUp6Hour("source", base); err == nil {
		t.Fatal("rollup on closed DB unexpectedly succeeded")
	}
	if _, err := store.ListEpisodes(base, base.Add(time.Hour)); err == nil {
		t.Fatal("list episodes on closed DB unexpectedly succeeded")
	}
	if _, err := store.Search(search); err == nil {
		t.Fatal("search on closed DB unexpectedly succeeded")
	}
	if _, err := store.GetReadItem(segment.ID); err == nil {
		t.Fatal("get read item on closed DB unexpectedly succeeded")
	}
	if _, err := store.Status(); err == nil {
		t.Fatal("status on closed DB unexpectedly succeeded")
	}
	if _, err := store.StageEpisode(episode.ID); err == nil {
		t.Fatal("stage episode on closed DB unexpectedly succeeded")
	}
	if _, err := store.PromoteEpisodeToCandidate(episode.ID); err == nil {
		t.Fatal("promote candidate on closed DB unexpectedly succeeded")
	}
	if _, err := store.PromoteEpisode(episode.ID); err == nil {
		t.Fatal("promote episode on closed DB unexpectedly succeeded")
	}
	if _, err := store.Expire(base); err == nil {
		t.Fatal("expire on closed DB unexpectedly succeeded")
	}
	if _, err := store.DeleteEpisode(episode.ID); err == nil {
		t.Fatal("delete episode on closed DB unexpectedly succeeded")
	}
	if _, err := store.ClearTimeRange(base, base.Add(time.Hour)); err == nil {
		t.Fatal("clear range on closed DB unexpectedly succeeded")
	}
	if _, err := store.ClearAll(); err == nil {
		t.Fatal("clear all on closed DB unexpectedly succeeded")
	}
	if _, err := store.ClearLastSession(); err == nil {
		t.Fatal("clear last session on closed DB unexpectedly succeeded")
	}
}
