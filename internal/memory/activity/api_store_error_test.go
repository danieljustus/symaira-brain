package activity

import (
	"testing"
	"time"
)

func TestActivityAPIAndScannerErrors(t *testing.T) {
	base := time.Date(2026, 8, 28, 12, 0, 0, 0, time.UTC)
	t.Run("episode search query error", func(t *testing.T) {
		store, database, _ := newActivityStore(t, Options{Now: func() time.Time { return base }})
		if _, err := database.Conn().Exec("DROP TABLE activity_episodes"); err != nil {
			t.Fatal(err)
		}
		_, err := store.Search(SearchOptions{Query: "query", From: base, To: base.Add(time.Hour), Limit: 1, MaxTokens: 10, IncludeEpisodes: true})
		if err == nil {
			t.Fatal("search unexpectedly succeeded")
		}
	})
	t.Run("status episode count error", func(t *testing.T) {
		store, database, _ := newActivityStore(t, Options{Now: func() time.Time { return base }})
		if err := store.SaveSegment(Segment{ID: "status-segment", Source: "source", Granularity: Granularity10Min,
			StartedAt: base, EndedAt: base.Add(time.Minute), RedactedSummary: "summary"}); err != nil {
			t.Fatal(err)
		}
		if _, err := database.Conn().Exec("DROP TABLE activity_episodes"); err != nil {
			t.Fatal(err)
		}
		if _, err := store.Status(); err == nil {
			t.Fatal("status unexpectedly succeeded")
		}
	})
	t.Run("segment list scanner error", func(t *testing.T) {
		store, database, _ := newActivityStore(t, Options{Now: func() time.Time { return base }})
		if err := store.SaveSegment(Segment{ID: "bad-list-segment", Source: "source", Granularity: Granularity10Min,
			StartedAt: base, EndedAt: base.Add(time.Minute), Applications: []string{"Editor"}, RedactedSummary: "summary"}); err != nil {
			t.Fatal(err)
		}
		if _, err := database.Conn().Exec("UPDATE activity_segments SET applications = ? WHERE id = ?", "not-json", "bad-list-segment"); err != nil {
			t.Fatal(err)
		}
		if _, err := store.ListSegments(base.Add(-time.Minute), base.Add(time.Hour)); err == nil {
			t.Fatal("list segments unexpectedly decoded invalid JSON")
		}
	})
	t.Run("episode list scanner error", func(t *testing.T) {
		store, database, _ := newActivityStore(t, Options{Now: func() time.Time { return base }})
		episode := Episode{ID: "bad-list-episode", Title: "episode", StartedAt: base,
			EndedAt: base.Add(time.Hour), Confidence: 0.5}
		if err := store.SaveEpisode(episode); err != nil {
			t.Fatal(err)
		}
		if _, err := database.Conn().Exec("UPDATE activity_episodes SET sources = ? WHERE id = ?", "not-json", episode.ID); err != nil {
			t.Fatal(err)
		}
		if _, err := store.ListEpisodes(base.Add(-time.Minute), base.Add(2*time.Hour)); err == nil {
			t.Fatal("list episodes unexpectedly decoded invalid JSON")
		}
	})
}
