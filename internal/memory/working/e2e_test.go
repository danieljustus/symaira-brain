package working

import (
	"context"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/consolidation"
	"github.com/danieljustus/symaira-brain/internal/memory/contextassembler"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
	"github.com/danieljustus/symaira-brain/internal/memory/memory"
)

func TestE2E_WorkingMemoryLifecycle(t *testing.T) {
	database := helperWorkingDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	patternExtractor := extractor.NewPatternExtractor()

	attr := memory.Attribution{Author: "test-user", SessionID: "sess-1"}

	// 1. Create a working memory
	m, _, err := memory.Store(database, embeddings, patternExtractor,
		"Current task: implement feature X", "project", nil, false, attr, nil, "test",
		true, 24*time.Hour)
	if err != nil {
		t.Fatalf("Store working memory failed: %v", err)
	}
	if m.Tier != "working" {
		t.Fatalf("expected Tier=working, got %q", m.Tier)
	}
	if m.ExpiresAt == nil {
		t.Fatal("expected ExpiresAt to be set")
	}

	// 2. Verify it's visible in list
	memories, err := database.ListMemories("project", 0, 100)
	if err != nil {
		t.Fatalf("ListMemories failed: %v", err)
	}
	found := false
	for _, mem := range memories {
		if mem.ID == m.ID {
			found = true
			break
		}
	}
	if !found {
		t.Error("working memory should appear in ListMemories before expiry")
	}

	// 4. Verify it's visible in GetWorkingMemories
	workingMems, err := database.GetWorkingMemories("project", 10)
	if err != nil {
		t.Fatalf("GetWorkingMemories failed: %v", err)
	}
	found = false
	for _, wm := range workingMems {
		if wm.ID == m.ID {
			found = true
			break
		}
	}
	if !found {
		t.Error("working memory should appear in GetWorkingMemories before expiry")
	}

	// 5. Create an expired working memory
	pastExpiry := time.Now().UTC().Add(-1 * time.Hour)
	expired, _, err := memory.Store(database, embeddings, patternExtractor,
		"Old working task that is done", "project", nil, false, attr, nil, "test",
		false, 0) // not working — we'll set it manually
	if err != nil {
		t.Fatalf("Store expired memory failed: %v", err)
	}
	if err := database.SetMemoryTier(expired.ID, "working", &pastExpiry); err != nil {
		t.Fatalf("SetMemoryTier failed: %v", err)
	}

	// 6. Verify expired working memory is excluded from list
	memories, err = database.ListMemories("project", 0, 100)
	if err != nil {
		t.Fatalf("ListMemories failed: %v", err)
	}
	for _, mem := range memories {
		if mem.ID == expired.ID {
			t.Error("expired working memory should NOT appear in ListMemories")
		}
	}

	// 7. Compact: evict expired working memories
	engine := consolidation.NewEngine(database, embeddings, "", "", "", false, "chat", 0)
	evictor := NewEvictor(database, embeddings, engine, false)

	result, err := evictor.CompactWorkingMemories(context.Background(), false)
	if err != nil {
		t.Fatalf("CompactWorkingMemories failed: %v", err)
	}
	if result.ExpiredCount != 1 {
		t.Errorf("expected 1 expired, got %d", result.ExpiredCount)
	}
	if result.EvictedCount != 1 {
		t.Errorf("expected 1 evicted, got %d", result.EvictedCount)
	}

	// 8. Verify expired working memory is gone
	got, err := database.GetMemory(expired.ID)
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got != nil {
		t.Error("expired working memory should be deleted after compaction")
	}

	// 9. Verify active working memory still exists
	gotActive, err := database.GetMemory(m.ID)
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if gotActive == nil {
		t.Error("active working memory should still exist after compaction")
	}
}

func TestE2E_MaxItemsCapBehavior(t *testing.T) {
	database := helperWorkingDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	patternExtractor := extractor.NewPatternExtractor()

	attr := memory.Attribution{Author: "test-user", SessionID: "sess-1"}

	// Create 5 working memories
	for i := 0; i < 5; i++ {
		_, _, err := memory.Store(database, embeddings, patternExtractor,
			"Working memory task", "project", nil, false, attr, nil, "test",
			true, 24*time.Hour)
		if err != nil {
			t.Fatalf("Store failed: %v", err)
		}
	}

	// Verify all are returned by GetWorkingMemories
	workingMems, err := database.GetWorkingMemories("project", 10)
	if err != nil {
		t.Fatalf("GetWorkingMemories failed: %v", err)
	}
	if len(workingMems) != 5 {
		t.Errorf("expected 5 working memories, got %d", len(workingMems))
	}

	// Verify with limit 2
	workingMems, err = database.GetWorkingMemories("project", 2)
	if err != nil {
		t.Fatalf("GetWorkingMemories with limit failed: %v", err)
	}
	if len(workingMems) != 2 {
		t.Errorf("expected 2 working memories with limit, got %d", len(workingMems))
	}
}

func TestE2E_ContextAssemblyInclusion(t *testing.T) {
	database := helperWorkingDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	patternExtractor := extractor.NewPatternExtractor()

	attr := memory.Attribution{Author: "test-user", SessionID: "sess-1"}

	// Create a working memory
	_, _, err := memory.Store(database, embeddings, patternExtractor,
		"Current task: implement feature X", "project", nil, false, attr, nil, "test",
		true, 24*time.Hour)
	if err != nil {
		t.Fatalf("Store failed: %v", err)
	}

	// Create assembler with working memory enabled
	a := contextassembler.NewAssembler(database, embeddings, &cfg.Context)
	a.SetWorkingMemoryConfig(&cfg.WorkingMemory)

	ctx, err := a.Assemble("test query", "", "")
	if err != nil {
		t.Fatalf("Assemble failed: %v", err)
	}

	hasWorkingMem := false
	for _, p := range ctx.Pieces {
		if p.Layer == "working_memory" {
			hasWorkingMem = true
			break
		}
	}
	if !hasWorkingMem {
		t.Error("working memory should be included in assembled context")
	}
}
