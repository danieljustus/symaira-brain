package mcp

import (
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

// conflictKind is a canonical semantic kind accepted by SetGoverned.
const conflictKind = "reference"

func TestServerWiresConflictCheckerFromConfig(t *testing.T) {
	s := helperServer(t) // config.Defaults(): conflict.enabled=true
	if s.service.conflictChecker == nil {
		t.Fatal("expected the conflict checker to be wired when conflict.enabled defaults to true")
	}

	cfg := config.Defaults()
	cfg.Conflict.Enabled = false
	database := helperDB(t)
	srv := NewServer(database, nil, "test", cfg)
	if srv.service.conflictChecker != nil {
		t.Fatal("expected no conflict checker when conflict.enabled=false")
	}
}

func TestMemorySetLiveWriteDeduplicates(t *testing.T) {
	s := helperServer(t)
	content := "alice prefers dark mode in all applications"

	id1, err := s.service.SetGoverned(content, "global", nil, "sess-1", "client-a", nil, "mcp", false, 0, conflictKind, false)
	if err != nil {
		t.Fatalf("first SetGoverned: %v", err)
	}
	id2, err := s.service.SetGoverned(content, "global", nil, "sess-1", "client-a", nil, "mcp", false, 0, conflictKind, false)
	if err != nil {
		t.Fatalf("second SetGoverned: %v", err)
	}
	if id2 != id1 {
		t.Errorf("byte-identical live write must dedup to the existing row: got %s, want %s", id2, id1)
	}
}

func TestMemorySetStagedWriteBypassesConflictCheck(t *testing.T) {
	s := helperServer(t)
	content := "alice prefers dark mode in all applications"

	id1, err := s.service.SetGoverned(content, "global", nil, "sess-1", "client-a", nil, "mcp", false, 0, conflictKind, false)
	if err != nil {
		t.Fatalf("live SetGoverned: %v", err)
	}
	// A staged candidate must never be deduplicated against a live fact:
	// it is a review candidate, not a duplicate assertion.
	id2, err := s.service.SetGoverned(content, "global", nil, "sess-2", "client-b", nil, "mcp", false, 0, conflictKind, true)
	if err != nil {
		t.Fatalf("staged SetGoverned: %v", err)
	}
	if id2 == id1 {
		t.Error("staged writes must bypass the conflict check and create their own row")
	}
}

func TestMemorySetWorkingWriteBypassesConflictCheck(t *testing.T) {
	s := helperServer(t)
	content := "alice prefers dark mode in all applications"

	id1, err := s.service.SetGoverned(content, "global", nil, "sess-1", "client-a", nil, "mcp", false, 0, conflictKind, false)
	if err != nil {
		t.Fatalf("live SetGoverned: %v", err)
	}
	id2, err := s.service.SetGoverned(content, "global", nil, "sess-2", "client-b", nil, "mcp", true, time.Hour, conflictKind, false)
	if err != nil {
		t.Fatalf("working SetGoverned: %v", err)
	}
	if id2 == id1 {
		t.Error("working memories must bypass the conflict check and create their own row")
	}
}
