package working

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/consolidation"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
)

func helperWorkingDB(t *testing.T) *db.DB {
	t.Helper()
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	t.Cleanup(func() { _ = database.Close() })
	return database
}

func TestCompactWorkingMemories_NoExpired(t *testing.T) {
	database := helperWorkingDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	engine := consolidation.NewEngine(database, embeddings, "", "", "", false, "chat", 0)

	ev := NewEvictor(database, embeddings, engine, false)

	futureExpiry := time.Now().UTC().Add(24 * time.Hour)
	if err := database.SaveMemory(&db.Memory{
		ID:        "wm-active",
		Content:   "Active working memory",
		Scope:     "project",
		Tier:      "working",
		ExpiresAt: &futureExpiry,
		Metadata:  map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory failed: %v", err)
	}

	result, err := ev.CompactWorkingMemories(context.Background(), true)
	if err != nil {
		t.Fatalf("CompactWorkingMemories failed: %v", err)
	}

	if result.ExpiredCount != 0 {
		t.Errorf("expected 0 expired, got %d", result.ExpiredCount)
	}
	if result.EvictedCount != 0 {
		t.Errorf("expected 0 evicted, got %d", result.EvictedCount)
	}
}

func TestCompactWorkingMemories_DryRun(t *testing.T) {
	database := helperWorkingDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	engine := consolidation.NewEngine(database, embeddings, "", "", "", false, "chat", 0)

	ev := NewEvictor(database, embeddings, engine, false)

	pastExpiry := time.Now().UTC().Add(-1 * time.Hour)
	if err := database.SaveMemory(&db.Memory{
		ID:        "wm-expired",
		Content:   "Expired working memory",
		Scope:     "project",
		Tier:      "working",
		ExpiresAt: &pastExpiry,
		Metadata:  map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory failed: %v", err)
	}

	result, err := ev.CompactWorkingMemories(context.Background(), true)
	if err != nil {
		t.Fatalf("CompactWorkingMemories failed: %v", err)
	}

	if result.ExpiredCount != 1 {
		t.Errorf("expected 1 expired, got %d", result.ExpiredCount)
	}
	if result.EvictedCount != 0 {
		t.Errorf("expected 0 evicted in dry run, got %d", result.EvictedCount)
	}

	got, err := database.GetMemory("wm-expired")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got == nil {
		t.Error("expired memory should NOT be deleted in dry run")
	}
}

func TestCompactWorkingMemories_RealRun(t *testing.T) {
	database := helperWorkingDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	engine := consolidation.NewEngine(database, embeddings, "", "", "", false, "chat", 0)

	ev := NewEvictor(database, embeddings, engine, false)

	pastExpiry := time.Now().UTC().Add(-1 * time.Hour)
	if err := database.SaveMemory(&db.Memory{
		ID:        "wm-expired",
		Content:   "Expired working memory",
		Scope:     "project",
		Tier:      "working",
		ExpiresAt: &pastExpiry,
		Metadata:  map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory failed: %v", err)
	}

	result, err := ev.CompactWorkingMemories(context.Background(), false)
	if err != nil {
		t.Fatalf("CompactWorkingMemories failed: %v", err)
	}

	if result.ExpiredCount != 1 {
		t.Errorf("expected 1 expired, got %d", result.ExpiredCount)
	}
	if result.EvictedCount != 1 {
		t.Errorf("expected 1 evicted, got %d", result.EvictedCount)
	}

	got, err := database.GetMemory("wm-expired")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got != nil {
		t.Error("expired memory should be deleted after real compaction")
	}
}

func TestEvictExpiredWorkingMemories(t *testing.T) {
	database := helperWorkingDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	engine := consolidation.NewEngine(database, embeddings, "", "", "", false, "chat", 0)

	ev := NewEvictor(database, embeddings, engine, false)

	pastExpiry := time.Now().UTC().Add(-1 * time.Hour)
	futureExpiry := time.Now().UTC().Add(24 * time.Hour)

	if err := database.SaveMemory(&db.Memory{
		ID: "expired-1", Content: "Expired", Scope: "project",
		Tier: "working", ExpiresAt: &pastExpiry, Metadata: map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory failed: %v", err)
	}
	if err := database.SaveMemory(&db.Memory{
		ID: "active-1", Content: "Active", Scope: "project",
		Tier: "working", ExpiresAt: &futureExpiry, Metadata: map[string]string{},
	}); err != nil {
		t.Fatalf("SaveMemory failed: %v", err)
	}

	count, err := ev.EvictExpired()
	if err != nil {
		t.Fatalf("EvictExpired failed: %v", err)
	}
	if count != 1 {
		t.Errorf("expected 1 eviction, got %d", count)
	}

	got, err := database.GetMemory("expired-1")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got != nil {
		t.Error("expired memory should be deleted")
	}
}

// Test error path when GetExpiredWorkingMemories fails (closed database).
func TestCompactWorkingMemories_GetExpiredError(t *testing.T) {
	database := helperWorkingDB(t)
	_ = database.Close() // close so SQL queries fail

	ev := NewEvictor(database, nil, nil, false)
	_, err := ev.CompactWorkingMemories(context.Background(), false)
	if err == nil {
		t.Fatal("expected error from closed database")
	}
	if !strings.Contains(err.Error(), "fetch expired working memories") {
		t.Errorf("expected error wrapping 'fetch expired working memories', got: %v", err)
	}
}

// Test error path when the consolidation engine fails (multiple expired
// memories in the same scope without an LLM endpoint → LLM call fails).
func TestCompactWorkingMemories_MultiConsolidationError(t *testing.T) {
	database := helperWorkingDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	engine := consolidation.NewEngine(database, embeddings, "", "", "", false, "chat", 0)

	ev := NewEvictor(database, embeddings, engine, false)

	pastExpiry := time.Now().UTC().Add(-1 * time.Hour)
	for i := 0; i < 2; i++ {
		id := fmt.Sprintf("wm-multi-err-%d", i)
		if err := database.SaveMemory(&db.Memory{
			ID: id, Content: fmt.Sprintf("Expired memory %d", i),
			Scope: "same-scope", Tier: "working",
			ExpiresAt: &pastExpiry, Metadata: map[string]string{},
		}); err != nil {
			t.Fatalf("SaveMemory failed: %v", err)
		}
	}

	_, err := ev.CompactWorkingMemories(context.Background(), false)
	if err == nil {
		t.Fatal("expected consolidation error (no LLM available)")
	}
	if !strings.Contains(err.Error(), "consolidate expired working memories") {
		t.Errorf("expected error wrapping 'consolidate expired working memories', got: %v", err)
	}
}

// Test dry-run behavior with multiple expired memories (different scopes,
// each scope has ≤1 memory so no LLM call needed).
func TestCompactWorkingMemories_MultiDryRunDiffScope(t *testing.T) {
	database := helperWorkingDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	engine := consolidation.NewEngine(database, embeddings, "", "", "", false, "chat", 0)

	ev := NewEvictor(database, embeddings, engine, false)

	pastExpiry := time.Now().UTC().Add(-1 * time.Hour)
	for i := 0; i < 3; i++ {
		id := fmt.Sprintf("wm-dry-%d", i)
		if err := database.SaveMemory(&db.Memory{
			ID: id, Content: fmt.Sprintf("Scope %d memory", i),
			Scope: fmt.Sprintf("scope-%d", i), Tier: "working",
			ExpiresAt: &pastExpiry, Metadata: map[string]string{},
		}); err != nil {
			t.Fatalf("SaveMemory failed: %v", err)
		}
	}

	// Dry-run: consolidation engine processes but does not persist;
	// evictor skips eviction because dryRun=true.
	result, err := ev.CompactWorkingMemories(context.Background(), true)
	if err != nil {
		t.Fatalf("CompactWorkingMemories(dryRun=true) failed: %v", err)
	}
	if result.ExpiredCount != 3 {
		t.Errorf("expected 3 expired, got %d", result.ExpiredCount)
	}
	if result.EvictedCount != 0 {
		t.Errorf("expected 0 evicted in dry run, got %d", result.EvictedCount)
	}

	// Memories must still be present after dry run.
	for i := 0; i < 3; i++ {
		id := fmt.Sprintf("wm-dry-%d", i)
		got, err := database.GetMemory(id)
		if err != nil {
			t.Fatalf("GetMemory %s failed: %v", id, err)
		}
		if got == nil {
			t.Errorf("memory %s should still exist after dry run", id)
		}
	}
}

// Test real (non-dry) run with multiple expired memories in different
// scopes.  Each scope gets exactly one memory so the consolidation engine
// takes the single-memory-per-scope fast path (no LLM required).
func TestCompactWorkingMemories_MultiRealRunDiffScope(t *testing.T) {
	database := helperWorkingDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	engine := consolidation.NewEngine(database, embeddings, "", "", "", false, "chat", 0)

	ev := NewEvictor(database, embeddings, engine, false)

	pastExpiry := time.Now().UTC().Add(-1 * time.Hour)
	for i := 0; i < 3; i++ {
		id := fmt.Sprintf("wm-real-%d", i)
		if err := database.SaveMemory(&db.Memory{
			ID: id, Content: fmt.Sprintf("Scope %d memory", i),
			Scope: fmt.Sprintf("scope-%d", i), Tier: "working",
			ExpiresAt: &pastExpiry, Metadata: map[string]string{},
		}); err != nil {
			t.Fatalf("SaveMemory failed: %v", err)
		}
	}

	result, err := ev.CompactWorkingMemories(context.Background(), false)
	if err != nil {
		t.Fatalf("CompactWorkingMemories failed: %v", err)
	}
	if result.ExpiredCount != 3 {
		t.Errorf("expected 3 expired, got %d", result.ExpiredCount)
	}
	if result.EvictedCount != 3 {
		t.Errorf("expected 3 evicted, got %d", result.EvictedCount)
	}

	// Memories must be gone after real compaction.
	for i := 0; i < 3; i++ {
		id := fmt.Sprintf("wm-real-%d", i)
		got, err := database.GetMemory(id)
		if err != nil {
			t.Fatalf("GetMemory %s failed: %v", id, err)
		}
		if got != nil {
			t.Errorf("memory %s should be deleted after real compaction, got status=%q",
				id, got.ConsolidationStatus)
		}
	}
}

// Test real run with multiple expired memories in the same scope, using a
// mock LLM endpoint so the consolidation engine can merge them.
func TestCompactWorkingMemories_MultiRealRunSameScope(t *testing.T) {
	database := helperWorkingDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)

	mockLLM := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]string{
			"response": `{
				"consolidated": [
					{
						"content": "Consolidated working memory summary.",
						"replaces_ids": ["wm-multi-real-0", "wm-multi-real-1"],
						"metadata": {"topic": "coverage-test"}
					}
				],
				"discarded_ids": []
			}`,
		})
	}))
	defer mockLLM.Close()

	engine := consolidation.NewEngine(database, embeddings, mockLLM.URL, "test-model", "ollama", false, "chat", 0)
	ev := NewEvictor(database, embeddings, engine, false)

	pastExpiry := time.Now().UTC().Add(-1 * time.Hour)
	for i := 0; i < 2; i++ {
		id := fmt.Sprintf("wm-multi-real-%d", i)
		if err := database.SaveMemory(&db.Memory{
			ID: id, Content: fmt.Sprintf("Memory %d for same-scope compaction", i),
			Scope: "shared-scope", Tier: "working",
			ExpiresAt: &pastExpiry, Metadata: map[string]string{},
		}); err != nil {
			t.Fatalf("SaveMemory failed: %v", err)
		}
	}

	result, err := ev.CompactWorkingMemories(context.Background(), false)
	if err != nil {
		t.Fatalf("CompactWorkingMemories failed: %v", err)
	}
	if result.ExpiredCount != 2 {
		t.Errorf("expected 2 expired, got %d", result.ExpiredCount)
	}
	if result.EvictedCount != 2 {
		t.Errorf("expected 2 evicted, got %d", result.EvictedCount)
	}

	// The two source memories  should be gone after compaction.
	for i := 0; i < 2; i++ {
		id := fmt.Sprintf("wm-multi-real-%d", i)
		got, err := database.GetMemory(id)
		if err != nil {
			t.Fatalf("GetMemory %s failed: %v", id, err)
		}
		if got != nil {
			t.Errorf("original memory %s should be deleted after compaction, got status=%q",
				id, got.ConsolidationStatus)
		}
	}
}
