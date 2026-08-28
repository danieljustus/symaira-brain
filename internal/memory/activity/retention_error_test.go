package activity

import (
	"testing"
	"time"

	memorydb "github.com/danieljustus/symaira-brain/internal/memory/db"
)

func TestActivityRetentionTransactionErrors(t *testing.T) {
	base := time.Date(2026, 8, 28, 12, 0, 0, 0, time.UTC)
	newStore := func(t *testing.T) (*Store, *memorydb.DB) {
		t.Helper()
		store, database, _ := newActivityStore(t, Options{Now: func() time.Time { return base }})
		return store, database
	}
	newSegment := Segment{ID: "retention-segment", Source: "source", Granularity: Granularity10Min,
		StartedAt: base, EndedAt: base.Add(time.Minute), RedactedSummary: "summary"}
	newEpisode := Episode{ID: "retention-episode", Title: "episode", StartedAt: base,
		EndedAt: base.Add(time.Hour), Confidence: 0.5, SegmentIDs: []string{newSegment.ID}}

	t.Run("expire episode delete fails", func(t *testing.T) {
		store, database := newStore(t)
		if _, err := database.Conn().Exec("DROP TABLE activity_episodes"); err != nil {
			t.Fatal(err)
		}
		if _, err := store.Expire(base); err == nil {
			t.Fatal("expire unexpectedly succeeded")
		}
	})
	t.Run("expire segment delete fails", func(t *testing.T) {
		store, database := newStore(t)
		if _, err := database.Conn().Exec("DROP TABLE activity_segments"); err != nil {
			t.Fatal(err)
		}
		if _, err := store.Expire(base); err == nil {
			t.Fatal("expire unexpectedly succeeded")
		}
	})
	t.Run("delete episode segment delete fails", func(t *testing.T) {
		store, database := newStore(t)
		if err := store.SaveSegment(newSegment); err != nil {
			t.Fatal(err)
		}
		if err := store.SaveEpisode(newEpisode); err != nil {
			t.Fatal(err)
		}
		if _, err := database.Conn().Exec("DROP TABLE activity_segments"); err != nil {
			t.Fatal(err)
		}
		if _, err := store.DeleteEpisode(newEpisode.ID); err == nil {
			t.Fatal("delete episode unexpectedly succeeded")
		}
	})
	t.Run("clear range segment delete fails", func(t *testing.T) {
		store, database := newStore(t)
		if _, err := database.Conn().Exec("DROP TABLE activity_segments"); err != nil {
			t.Fatal(err)
		}
		if _, err := store.ClearTimeRange(base, base.Add(time.Hour)); err == nil {
			t.Fatal("clear range unexpectedly succeeded")
		}
	})
	t.Run("clear all segment delete fails", func(t *testing.T) {
		store, database := newStore(t)
		if _, err := database.Conn().Exec("DROP TABLE activity_segments"); err != nil {
			t.Fatal(err)
		}
		if _, err := store.ClearAll(); err == nil {
			t.Fatal("clear all unexpectedly succeeded")
		}
	})
}
