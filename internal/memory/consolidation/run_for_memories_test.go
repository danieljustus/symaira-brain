package consolidation

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
)

func TestRunConsolidationForMemories_Empty(t *testing.T) {
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	engine := NewEngine(nil, embeddings, "", "", "", false, "chat", 0)

	summaries, err := engine.RunConsolidationForMemories(context.Background(), nil, false)
	if err != nil {
		t.Fatalf("expected no error for empty input, got %v", err)
	}
	if summaries != nil {
		t.Errorf("expected nil summaries for empty input, got %v", summaries)
	}
}

func TestRunConsolidationForMemories_SingleMemoryMarksConsolidated(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-forgroup-single-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	cfg := config.Defaults()
	cfg.Database.Path = filepath.Join(tempDir, "test.db")
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	m := &db.Memory{ID: "wm-1", Content: "solo working memory", Scope: "session-a", ConsolidationStatus: "raw", Metadata: map[string]string{}}
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("seed SaveMemory failed: %v", err)
	}

	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	engine := NewEngine(database, embeddings, "", "", "", false, "chat", 0)

	summaries, err := engine.RunConsolidationForMemories(context.Background(), []*db.Memory{m}, false)
	if err != nil {
		t.Fatalf("RunConsolidationForMemories failed: %v", err)
	}
	if len(summaries) != 1 {
		t.Fatalf("expected 1 scope summary, got %d", len(summaries))
	}
	if summaries[0].Scope != "session-a" {
		t.Errorf("expected scope session-a, got %s", summaries[0].Scope)
	}
	if len(summaries[0].NewMemories) != 1 || summaries[0].ReplacedIDToNewID["wm-1"] != "wm-1" {
		t.Errorf("expected single memory to be marked consolidated in place, got %+v", summaries[0])
	}

	updated, err := database.GetMemory("wm-1")
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if updated.ConsolidationStatus != "consolidated" {
		t.Errorf("expected wm-1 status 'consolidated', got %q", updated.ConsolidationStatus)
	}
}

func TestRunConsolidationForMemories_MultiMemoryUsesLLMAndGroupsByScope(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-forgroup-multi-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	defer os.RemoveAll(tempDir)

	cfg := config.Defaults()
	cfg.Database.Path = filepath.Join(tempDir, "test.db")
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	defer func() { _ = database.Close() }()

	m1 := &db.Memory{ID: "wm-a1", Content: "Daniel likes dark mode", Scope: "session-a", ConsolidationStatus: "raw", Metadata: map[string]string{}}
	m2 := &db.Memory{ID: "wm-a2", Content: "Daniel prefers dark backgrounds", Scope: "session-a", ConsolidationStatus: "raw", Metadata: map[string]string{}}
	m3 := &db.Memory{ID: "wm-b1", Content: "unrelated other-scope memory", Scope: "session-b", ConsolidationStatus: "raw", Metadata: map[string]string{}}
	for _, m := range []*db.Memory{m1, m2, m3} {
		if err := database.SaveMemory(m); err != nil {
			t.Fatalf("seed SaveMemory %s failed: %v", m.ID, err)
		}
	}

	mockLLM := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		respObj := map[string]string{
			"response": `{
				"consolidated": [
					{
						"content": "Daniel prefers dark mode.",
						"replaces_ids": ["1", "2"],
						"metadata": {}
					}
				],
				"discarded_ids": []
			}`,
		}
		_ = json.NewEncoder(w).Encode(respObj)
	}))
	defer mockLLM.Close()

	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	engine := NewEngine(database, embeddings, mockLLM.URL, "test-model", "ollama", false, "chat", 0)

	summaries, err := engine.RunConsolidationForMemories(context.Background(), []*db.Memory{m1, m2, m3}, false)
	if err != nil {
		t.Fatalf("RunConsolidationForMemories failed: %v", err)
	}
	if len(summaries) != 2 {
		t.Fatalf("expected 2 scope summaries (session-a via LLM, session-b single), got %d", len(summaries))
	}

	scopes := map[string]ScopeChangeSummary{}
	for _, s := range summaries {
		scopes[s.Scope] = s
	}

	sessionA, ok := scopes["session-a"]
	if !ok {
		t.Fatalf("expected a session-a summary, got %+v", summaries)
	}
	if len(sessionA.ArchivedMemoryIDs) != 2 {
		t.Errorf("expected wm-a1 and wm-a2 archived for session-a, got %v", sessionA.ArchivedMemoryIDs)
	}

	a1, err := database.GetMemory("wm-a1")
	if err != nil {
		t.Fatalf("GetMemory wm-a1 failed: %v", err)
	}
	if a1.ConsolidationStatus != "archived" {
		t.Errorf("expected wm-a1 archived, got %q", a1.ConsolidationStatus)
	}

	sessionB, ok := scopes["session-b"]
	if !ok {
		t.Fatalf("expected a session-b summary, got %+v", summaries)
	}
	if len(sessionB.NewMemories) != 1 {
		t.Errorf("expected wm-b1 alone marked consolidated, got %+v", sessionB)
	}
	b1, err := database.GetMemory("wm-b1")
	if err != nil {
		t.Fatalf("GetMemory wm-b1 failed: %v", err)
	}
	if b1.ConsolidationStatus != "consolidated" {
		t.Errorf("expected wm-b1 status 'consolidated', got %q", b1.ConsolidationStatus)
	}
}
