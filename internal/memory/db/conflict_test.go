package db

import (
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

func TestFindActiveDuplicate(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatalf("Open failed: %v", err)
	}
	defer func() { _ = database.Close() }()

	now := time.Now().UTC()
	content := "the daemon runs on port 8787"
	hash := ComputeContentHash(content)

	save := func(id, scope string, createdAt time.Time) *Memory {
		m := &Memory{
			ID:              id,
			Content:         content,
			Scope:           scope,
			Metadata:        map[string]string{},
			Embedding:       embeddingVector(1.0),
			EmbeddingSource: "test",
			CreatedAt:       createdAt,
		}
		if err := database.SaveMemory(m); err != nil {
			t.Fatalf("SaveMemory(%s): %v", id, err)
		}
		return m
	}

	// No rows yet.
	got, err := database.FindActiveDuplicate(hash, "global")
	if err != nil {
		t.Fatalf("FindActiveDuplicate: %v", err)
	}
	if got != nil {
		t.Fatalf("expected nil before any insert, got %+v", got)
	}

	// Insert in the global scope.
	older := save("global-older", "global", now.Add(-2*time.Hour))
	newer := save("global-newer", "global", now.Add(-time.Hour))

	got, err = database.FindActiveDuplicate(hash, "global")
	if err != nil {
		t.Fatalf("FindActiveDuplicate: %v", err)
	}
	if got == nil || got.ID != newer.ID {
		t.Fatalf("expected newest duplicate %s, got %+v", newer.ID, got)
	}
	_ = older

	// Scope is respected: a duplicate in another scope is not returned.
	if got, err = database.FindActiveDuplicate(hash, "project"); err != nil {
		t.Fatalf("FindActiveDuplicate: %v", err)
	} else if got != nil {
		t.Fatalf("expected nil for a different scope, got %+v", got)
	}

	// Superseded rows are not dedup targets.
	if err := database.SupersedeMemory(newer.ID, "winner-1", now, "actor", "sess"); err != nil {
		t.Fatalf("SupersedeMemory: %v", err)
	}
	got, err = database.FindActiveDuplicate(hash, "global")
	if err != nil {
		t.Fatalf("FindActiveDuplicate: %v", err)
	}
	if got == nil || got.ID != older.ID {
		t.Fatalf("expected fallback to the non-superseded duplicate %s, got %+v", older.ID, got)
	}

	// Empty hash never matches.
	if got, err = database.FindActiveDuplicate("", "global"); err != nil {
		t.Fatalf("FindActiveDuplicate: %v", err)
	} else if got != nil {
		t.Fatalf("expected nil for empty hash, got %+v", got)
	}
}

func TestSupersedeMemory(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatalf("Open failed: %v", err)
	}
	defer func() { _ = database.Close() }()

	now := time.Now().UTC()
	loser := &Memory{
		ID:              "loser-1",
		Content:         "the daemon listens on port 8787",
		Scope:           "global",
		Metadata:        map[string]string{},
		Embedding:       embeddingVector(1.0),
		EmbeddingSource: "test",
		CreatedAt:       now.Add(-time.Hour),
	}
	if err := database.SaveMemory(loser); err != nil {
		t.Fatalf("SaveMemory: %v", err)
	}

	validTo := now
	if err := database.SupersedeMemory("loser-1", "winner-1", validTo, "actor-b", "sess-b"); err != nil {
		t.Fatalf("SupersedeMemory: %v", err)
	}

	got, err := database.loadMemory("loser-1")
	if err != nil {
		t.Fatalf("loadMemory: %v", err)
	}
	if got.SupersededBy != "winner-1" {
		t.Errorf("superseded_by = %q, want winner-1", got.SupersededBy)
	}
	if got.ValidTo == nil || !got.ValidTo.UTC().Truncate(time.Second).Equal(validTo.UTC().Truncate(time.Second)) {
		t.Errorf("valid_to not closed at winner creation time: %v", got.ValidTo)
	}
	if got.UpdatedBy != "actor-b" {
		t.Errorf("updated_by = %q, want actor-b", got.UpdatedBy)
	}

	// Guard: an already-superseded row is never re-litigated.
	if err := database.SupersedeMemory("loser-1", "winner-2", now, "actor-c", "sess-c"); err != nil {
		t.Fatalf("SupersedeMemory: %v", err)
	}
	got, _ = database.loadMemory("loser-1")
	if got.SupersededBy != "winner-1" {
		t.Errorf("superseded_by must stay winner-1 after a second call, got %q", got.SupersededBy)
	}

	// Missing loser is a no-op, not an error.
	if err := database.SupersedeMemory("does-not-exist", "winner-1", now, "actor", "sess"); err != nil {
		t.Fatalf("SupersedeMemory on a missing loser must not error: %v", err)
	}
}
