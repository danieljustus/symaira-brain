package db

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

func TestListMemoriesLiteWithCursor_NoDuplicatesNoSkips(t *testing.T) {
	tempDir := t.TempDir()
	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHome)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	// Seed memories with a shared timestamp at a boundary.
	shared := time.Date(2026, 1, 2, 0, 0, 0, 0, time.UTC)
	mems := []*Memory{
		{ID: "m1", Content: "A", Scope: "global", CreatedAt: shared.Add(-2 * time.Hour), ReviewStatus: ReviewApproved},
		{ID: "m2", Content: "B", Scope: "global", CreatedAt: shared.Add(-1 * time.Hour), ReviewStatus: ReviewApproved},
		{ID: "m3", Content: "C", Scope: "global", CreatedAt: shared, ReviewStatus: ReviewApproved},
		{ID: "m4", Content: "D", Scope: "global", CreatedAt: shared, ReviewStatus: ReviewApproved},
		{ID: "m5", Content: "E", Scope: "global", CreatedAt: shared, ReviewStatus: ReviewApproved},
		{ID: "m6", Content: "F", Scope: "global", CreatedAt: shared.Add(1 * time.Hour), ReviewStatus: ReviewApproved},
		{ID: "m7", Content: "G", Scope: "global", CreatedAt: shared.Add(2 * time.Hour), ReviewStatus: ReviewApproved},
	}
	for _, m := range mems {
		if err := database.SaveMemory(m); err != nil {
			t.Fatalf("save %s: %v", m.ID, err)
		}
	}

	pageSize := 3
	page1, err := database.ListMemoriesLiteWithCursor("global", nil, pageSize+1)
	if err != nil {
		t.Fatalf("page 1: %v", err)
	}
	// DB returns up to limit; the handler truncates to pageSize.
	if len(page1) > pageSize+1 {
		t.Fatalf("page 1 returned %d, want <= %d", len(page1), pageSize+1)
	}
	if len(page1) != pageSize+1 {
		t.Fatalf("page 1 returned %d, want %d (to detect next page)", len(page1), pageSize+1)
	}

	// Simulate handler truncation.
	atEnd := len(page1) <= pageSize
	if !atEnd {
		page1 = page1[:pageSize]
	}

	// Build cursor from last item of page 1.
	last := page1[len(page1)-1]
	var cursor *MemoryCursor
	if last.ID != "" {
		cursor = &MemoryCursor{Timestamp: last.CreatedAt, ID: last.ID}
	}

	page2, err := database.ListMemoriesLiteWithCursor("global", cursor, pageSize+1)
	if err != nil {
		t.Fatalf("page 2: %v", err)
	}
	if len(page2) > pageSize+1 {
		t.Fatalf("page 2 returned %d, want <= %d", len(page2), pageSize+1)
	}

	// Collect IDs.
	ids1 := make([]string, len(page1))
	for i, m := range page1 {
		ids1[i] = m.ID
	}
	ids2 := make([]string, len(page2))
	for i, m := range page2 {
		ids2[i] = m.ID
	}

	// Disjoint.
	for _, id := range ids1 {
		for _, id2 := range ids2 {
			if id == id2 {
				t.Fatalf("duplicate %q across pages", id)
			}
		}
	}

	// Complete: union must contain all 7.
	seen := make(map[string]bool)
	for _, id := range ids1 {
		seen[id] = true
	}
	for _, id := range ids2 {
		seen[id] = true
	}
	if len(seen) != len(mems) {
		t.Fatalf("union has %d memories, want %d", len(seen), len(mems))
	}

	// Deterministic ordering: page1 should be newest, page2 older.
	for i := 1; i < len(page1); i++ {
		if page1[i].CreatedAt.After(page1[i-1].CreatedAt) {
			t.Fatalf("page1 not sorted DESC at %d", i)
		}
		if page1[i].CreatedAt.Equal(page1[i-1].CreatedAt) && page1[i].ID > page1[i-1].ID {
			t.Fatalf("page1 not sorted by id DESC at %d", i)
		}
	}
	if len(page2) > 0 {
		for i := 1; i < len(page2); i++ {
			if page2[i].CreatedAt.After(page2[i-1].CreatedAt) {
				t.Fatalf("page2 not sorted DESC at %d", i)
			}
			if page2[i].CreatedAt.Equal(page2[i-1].CreatedAt) && page2[i].ID > page2[i-1].ID {
				t.Fatalf("page2 not sorted by id DESC at %d", i)
			}
		}
		// Last of page1 should be >= first of page2 (cursor boundary).
		if last.CreatedAt.Before(page2[0].CreatedAt) {
			t.Fatalf("page2 contains newer memory than page1 cursor")
		}
		if last.CreatedAt.Equal(page2[0].CreatedAt) && last.ID < page2[0].ID {
			t.Fatalf("page2 contains same-timestamp memory with higher id than page1 cursor")
		}
	}
}

func TestListMemoriesLiteWithCursor_LegacyCursorCompatibility(t *testing.T) {
	tempDir := t.TempDir()
	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	defer os.Setenv("HOME", oldHome)

	database, err := Open(config.Defaults())
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	ts := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	m1 := &Memory{ID: "legacy-1", Content: "old", Scope: "global", CreatedAt: ts.Add(-1 * time.Hour), ReviewStatus: ReviewApproved}
	m2 := &Memory{ID: "legacy-2", Content: "new", Scope: "global", CreatedAt: ts.Add(1 * time.Hour), ReviewStatus: ReviewApproved}
	for _, m := range []*Memory{m1, m2} {
		if err := database.SaveMemory(m); err != nil {
			t.Fatalf("save: %v", err)
		}
	}

	// Legacy cursor is raw RFC3339Nano timestamp.
	legacyCursor := &MemoryCursor{Timestamp: ts}
	results, err := database.ListMemoriesLiteWithCursor("global", legacyCursor, 10)
	if err != nil {
		t.Fatalf("ListMemoriesLiteWithCursor: %v", err)
	}
	if len(results) != 1 {
		t.Fatalf("expected 1 result with legacy cursor, got %d", len(results))
	}
	if results[0].ID != "legacy-1" {
		t.Fatalf("expected legacy-1, got %s", results[0].ID)
	}
}

func TestGetMemoriesSinceCursorIDForSyncExcludesMarkedActivity(t *testing.T) {
	tempDir := t.TempDir()
	cfg := config.Defaults()
	cfg.Database.Path = filepath.Join(tempDir, "sync.db")
	database, err := Open(cfg)
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	ts := time.Date(2026, 8, 28, 12, 0, 0, 0, time.UTC)
	for _, m := range []*Memory{
		{ID: "sync-normal", Content: "normal", Scope: "global", CreatedAt: ts, UpdatedAt: ts, Metadata: map[string]string{"source": "test"}, ReviewStatus: ReviewApproved},
		{ID: "sync-activity", Content: "activity", Scope: "global", CreatedAt: ts.Add(time.Minute), UpdatedAt: ts.Add(time.Minute), Metadata: map[string]string{"sync_exclude": "true"}, ReviewStatus: ReviewStaged},
	} {
		if err := database.SaveMemory(m); err != nil {
			t.Fatalf("save %s: %v", m.ID, err)
		}
	}

	memories, err := database.GetMemoriesSinceCursorIDForSync(time.Time{}, "", 10)
	if err != nil {
		t.Fatalf("GetMemoriesSinceCursorIDForSync: %v", err)
	}
	if len(memories) != 1 || memories[0].ID != "sync-normal" {
		t.Fatalf("sync memories = %#v, want only sync-normal", memories)
	}

	// The cursor form must keep the exclusion predicate outside both keyset
	// branches; otherwise a newer activity row bypasses the filter because SQL
	// AND binds more tightly than OR.
	memories, err = database.GetMemoriesSinceCursorIDForSync(memories[0].UpdatedAt, "sync-normal", 10)
	if err != nil {
		t.Fatalf("GetMemoriesSinceCursorIDForSync with cursor: %v", err)
	}
	if len(memories) != 0 {
		t.Fatalf("cursor sync memories = %#v, want no excluded activity rows", memories)
	}
}
