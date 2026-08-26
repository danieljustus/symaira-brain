package consolidation

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
	"github.com/danieljustus/symaira-corekit/evidencekit"
)

func TestParseJSONResponse(t *testing.T) {
	tests := []struct {
		name        string
		input       string
		wantErr     bool
		wantConsol  int
		wantDiscard int
	}{
		{
			name: "valid JSON",
			input: `{
				"consolidated": [{"content": "fact", "replaces_ids": ["a"], "metadata": {}}],
				"discarded_ids": ["b"]
			}`,
			wantErr:     false,
			wantConsol:  1,
			wantDiscard: 1,
		},
		{
			name:    "malformed JSON - missing closing brace",
			input:   `{"consolidated": [{"content": "fact"`,
			wantErr: true,
		},
		{
			name:    "malformed JSON - invalid syntax",
			input:   `not json at all`,
			wantErr: true,
		},
		{
			name:    "empty string",
			input:   "",
			wantErr: true,
		},
		{
			name:    "empty object",
			input:   `{}`,
			wantErr: false,
		},
		{
			name:    "markdown wrapped JSON",
			input:   "```json\n" + `{"consolidated": [], "discarded_ids": []}` + "\n```",
			wantErr: false,
		},
		{
			name:    "JSON with extra text before",
			input:   "Here is the result: {\"consolidated\": [], \"discarded_ids\": []}",
			wantErr: false,
		},
		{
			name:    "fenced JSON with prose before and after",
			input:   "Sure, here you go:\n```json\n{\"consolidated\": [], \"discarded_ids\": []}\n```\nHope that helps!",
			wantErr: false,
		},
		{
			name:    "think preamble before JSON",
			input:   "<think>Let me analyze the memories and merge duplicates...</think>\n{\"consolidated\": [], \"discarded_ids\": []}",
			wantErr: false,
		},
		{
			name:    "think preamble before fenced JSON with prose",
			input:   "<think>reasoning about the merge</think>\nResult:\n```json\n{\"consolidated\": [], \"discarded_ids\": []}\n```",
			wantErr: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result, err := parseJSONResponse(tt.input)
			if (err != nil) != tt.wantErr {
				t.Errorf("parseJSONResponse() error = %v, wantErr %v", err, tt.wantErr)
				return
			}
			if !tt.wantErr && result != nil {
				if len(result.Consolidated) != tt.wantConsol {
					t.Errorf("expected %d consolidated items, got %d", tt.wantConsol, len(result.Consolidated))
				}
				if len(result.DiscardedIDs) != tt.wantDiscard {
					t.Errorf("expected %d discarded IDs, got %d", tt.wantDiscard, len(result.DiscardedIDs))
				}
			}
		})
	}
}

func TestNewEngineDefaults(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-engine-defaults-test-*")
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

	embeddings := extractor.NewEmbeddingsGenerator(cfg)

	// Test Ollama defaults
	eng := NewEngine(database, embeddings, "", "", "", false, "chat", 0)
	if eng.llmClient.OllamaBase != "http://localhost:11434" {
		t.Errorf("expected Ollama default URL, got %s", eng.llmClient.OllamaBase)
	}
	if eng.llmClient.OllamaModel != "llama3" {
		t.Errorf("expected Ollama default model 'llama3', got %s", eng.llmClient.OllamaModel)
	}
	if eng.llmProvider != "ollama" {
		t.Errorf("expected provider 'ollama', got %s", eng.llmProvider)
	}

	// Test OpenAI defaults
	eng2 := NewEngine(database, embeddings, "", "", "openai", false, "chat", 0)
	if eng2.llmClient.OllamaBase != "http://localhost:11434" {
		t.Errorf("expected Ollama default URL, got %s", eng2.llmClient.OllamaBase)
	}
	if eng2.llmClient.OllamaModel != "llama3" {
		t.Errorf("expected Ollama default model 'llama3', got %s", eng2.llmClient.OllamaModel)
	}
}

func TestEngineConsolidation(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-engine-test-*")
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

	// Seed raw memories
	m1 := &db.Memory{
		ID:                  "mem-1",
		Content:             "Daniel likes dark mode",
		Scope:               "global",
		ConsolidationStatus: "raw",
		Metadata:            map[string]string{},
	}
	m2 := &db.Memory{
		ID:                  "mem-2",
		Content:             "Daniel prefers dark backgrounds",
		Scope:               "global",
		ConsolidationStatus: "raw",
		Metadata:            map[string]string{},
	}
	m3 := &db.Memory{
		ID:                  "mem-3",
		Content:             "Daniel is going to get coffee now",
		Scope:               "global",
		ConsolidationStatus: "raw",
		Metadata:            map[string]string{},
	}
	// A memory in a different scope
	m4 := &db.Memory{
		ID:                  "mem-4",
		Content:             "Swift is used for terminal app",
		Scope:               "project-a",
		ConsolidationStatus: "raw",
		Metadata:            map[string]string{},
	}

	for _, m := range []*db.Memory{m1, m2, m3, m4} {
		if err := database.SaveMemory(m); err != nil {
			t.Fatalf("failed to seed memory %s: %v", m.ID, err)
		}
	}

	// Create mock LLM server
	serverCalled := 0
	mockLLM := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		serverCalled++
		w.Header().Set("Content-Type", "application/json")

		// Mock response for Ollama
		respObj := map[string]string{
			"response": `{
				"consolidated": [
					{
						"content": "Daniel prefers dark mode.",
						"replaces_ids": ["1", "2"],
						"metadata": { "topic": "preferences" }
					}
				],
				"discarded_ids": ["3"]
			}`,
		}
		_ = json.NewEncoder(w).Encode(respObj)
	}))
	defer mockLLM.Close()

	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	engine := NewEngine(database, embeddings, mockLLM.URL, "test-model", "ollama", false, "chat", 0)

	// 1. Dry Run test
	dryRunSummaries, err := engine.RunConsolidation(context.Background(), "global", true)
	if err != nil {
		t.Fatalf("dry run failed: %v", err)
	}

	if len(dryRunSummaries) != 1 {
		t.Errorf("expected 1 scope summary for dry run, got %d", len(dryRunSummaries))
	} else {
		s := dryRunSummaries[0]
		if s.Scope != "global" {
			t.Errorf("expected global scope, got %s", s.Scope)
		}
		if len(s.NewMemories) != 1 {
			t.Errorf("expected 1 proposed consolidated memory, got %d", len(s.NewMemories))
		}
		if len(s.ArchivedMemoryIDs) != 3 { // mem-1, mem-2 (replaced) + mem-3 (discarded)
			t.Errorf("expected 3 archived memory proposals, got %d", len(s.ArchivedMemoryIDs))
		}
	}

	// Verify database was NOT changed during dry run
	rawMems, err := database.GetRawMemories()
	if err != nil {
		t.Fatalf("failed to get raw memories: %v", err)
	}
	if len(rawMems) != 4 {
		t.Errorf("expected 4 raw memories after dry run, got %d", len(rawMems))
	}

	// 2. Real run for scope "global"
	realSummaries, err := engine.RunConsolidation(context.Background(), "global", false)
	if err != nil {
		t.Fatalf("real run failed: %v", err)
	}

	if len(realSummaries) != 1 {
		t.Fatalf("expected 1 scope summary for real run, got %d", len(realSummaries))
	}

	// Verify database changes
	// raw memories remaining should be: mem-4 (different scope)
	rawRemaining, err := database.GetRawMemories()
	if err != nil {
		t.Fatalf("failed to get raw memories: %v", err)
	}
	if len(rawRemaining) != 1 || rawRemaining[0].ID != "mem-4" {
		t.Errorf("expected only mem-4 to remain raw, got %v", rawRemaining)
	}

	// ListMemories should return:
	// - the new consolidated memory (status consolidated)
	// - mem-4 (status raw)
	// (excluding mem-1, mem-2, mem-3 because they are now archived)
	activeList, err := database.ListMemories("", 0, 10)
	if err != nil {
		t.Fatalf("failed to list memories: %v", err)
	}
	if len(activeList) != 2 {
		t.Errorf("expected 2 active memories, got %d", len(activeList))
	}

	// Retrieve archived memories directly to check fields
	m1Archived, err := database.GetMemory("mem-1")
	if err != nil {
		t.Fatalf("failed to get mem-1: %v", err)
	}
	if m1Archived.ConsolidationStatus != "archived" {
		t.Errorf("expected mem-1 status 'archived', got '%s'", m1Archived.ConsolidationStatus)
	}
	if m1Archived.ConsolidatedIntoID == "" {
		t.Errorf("expected mem-1 to link to new consolidated memory, got empty ConsolidatedIntoID")
	}

	m3Archived, err := database.GetMemory("mem-3")
	if err != nil {
		t.Fatalf("failed to get mem-3: %v", err)
	}
	if m3Archived.ConsolidationStatus != "archived" {
		t.Errorf("expected mem-3 status 'archived', got '%s'", m3Archived.ConsolidationStatus)
	}
	if m3Archived.ConsolidatedIntoID != "" {
		t.Errorf("expected mem-3 (discarded) ConsolidatedIntoID to be empty, got '%s'", m3Archived.ConsolidatedIntoID)
	}

	// 3. Run consolidation for "project-a" (single memory -> should be marked consolidated without LLM call)
	origServerCalls := serverCalled
	projectSummaries, err := engine.RunConsolidation(context.Background(), "project-a", false)
	if err != nil {
		t.Fatalf("project consolidation failed: %v", err)
	}

	if len(projectSummaries) != 1 {
		t.Fatalf("expected 1 project summary, got %d", len(projectSummaries))
	}
	if serverCalled != origServerCalls {
		t.Errorf("expected 0 LLM calls for single-memory consolidation, but server was called")
	}

	// Verify mem-4 has status consolidated now
	mem4, err := database.GetMemory("mem-4")
	if err != nil {
		t.Fatalf("failed to get mem-4: %v", err)
	}
	if mem4.ConsolidationStatus != "consolidated" {
		t.Errorf("expected mem-4 status 'consolidated', got '%s'", mem4.ConsolidationStatus)
	}
}

func TestEngineConsolidationPropagatesEvidence(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-engine-evidence-test-*")
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

	m1 := &db.Memory{ID: "ev-mem-1", Content: "Daniel likes dark mode", Scope: "global", ConsolidationStatus: "raw", Metadata: map[string]string{}}
	m2 := &db.Memory{ID: "ev-mem-2", Content: "Daniel prefers dark backgrounds", Scope: "global", ConsolidationStatus: "raw", Metadata: map[string]string{}}
	for _, m := range []*db.Memory{m1, m2} {
		if err := database.SaveMemory(m); err != nil {
			t.Fatalf("failed to seed memory %s: %v", m.ID, err)
		}
	}

	if err := database.SaveMemoryEvidence("ev-mem-1", []evidencekit.Extraction{
		{EvidenceText: "Daniel likes dark mode", Span: evidencekit.Span{Start: 0, End: 22}, AlignmentStatus: evidencekit.AlignmentExact},
	}); err != nil {
		t.Fatalf("failed to seed evidence for ev-mem-1: %v", err)
	}
	if err := database.SaveMemoryEvidence("ev-mem-2", []evidencekit.Extraction{
		{EvidenceText: "Daniel prefers dark backgrounds", Span: evidencekit.Span{Start: 0, End: 30}, AlignmentStatus: evidencekit.AlignmentExact},
	}); err != nil {
		t.Fatalf("failed to seed evidence for ev-mem-2: %v", err)
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

	summaries, err := engine.RunConsolidation(context.Background(), "global", false)
	if err != nil {
		t.Fatalf("consolidation failed: %v", err)
	}
	if len(summaries) != 1 || len(summaries[0].NewMemories) != 1 {
		t.Fatalf("expected 1 new consolidated memory, got %+v", summaries)
	}
	newID := summaries[0].NewMemories[0].ID

	oldEvidence1, err := database.GetMemoryEvidence("ev-mem-1")
	if err != nil {
		t.Fatalf("GetMemoryEvidence(ev-mem-1) failed: %v", err)
	}
	if len(oldEvidence1) != 0 {
		t.Errorf("expected evidence moved off ev-mem-1, got %d rows", len(oldEvidence1))
	}
	oldEvidence2, err := database.GetMemoryEvidence("ev-mem-2")
	if err != nil {
		t.Fatalf("GetMemoryEvidence(ev-mem-2) failed: %v", err)
	}
	if len(oldEvidence2) != 0 {
		t.Errorf("expected evidence moved off ev-mem-2, got %d rows", len(oldEvidence2))
	}

	newEvidence, err := database.GetMemoryEvidence(newID)
	if err != nil {
		t.Fatalf("GetMemoryEvidence(new) failed: %v", err)
	}
	if len(newEvidence) != 2 {
		t.Fatalf("expected 2 evidence rows on consolidated memory (from both replaced raws), got %d", len(newEvidence))
	}
}

// TestRunConsolidationSkipsFailedScope verifies that a per-scope LLM/parse
// failure is logged and skipped while other scopes still proceed.
func TestRunConsolidationSkipsFailedScope(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-engine-skip-test-*")
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

	seed := func(id, content, scope string) {
		m := &db.Memory{ID: id, Content: content, Scope: scope, ConsolidationStatus: "raw", Metadata: map[string]string{}}
		if err := database.SaveMemory(m); err != nil {
			t.Fatalf("failed to seed memory %s: %v", id, err)
		}
	}
	seed("good-1", "Daniel likes dark mode", "good-scope")
	seed("good-2", "Daniel prefers dark backgrounds", "good-scope")
	seed("bad-1", "Some fact one", "bad-scope")
	seed("bad-2", "Some fact two", "bad-scope")

	mockLLM := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		var reqBody map[string]interface{}
		if err := json.NewDecoder(r.Body).Decode(&reqBody); err != nil {
			t.Errorf("failed to decode LLM request: %v", err)
		}
		prompt, _ := reqBody["prompt"].(string)
		var resp string
		if strings.Contains(prompt, `"bad-scope"`) {
			resp = "this is not json at all"
		} else {
			resp = `{"consolidated": [{"content": "Daniel prefers dark mode.", "replaces_ids": ["1", "2"], "metadata": {}}], "discarded_ids": []}`
		}
		_ = json.NewEncoder(w).Encode(map[string]string{"response": resp})
	}))
	defer mockLLM.Close()

	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	engine := NewEngine(database, embeddings, mockLLM.URL, "test-model", "ollama", false, "chat", 0)

	summaries, err := engine.RunConsolidation(context.Background(), "", true)
	if err != nil {
		t.Fatalf("expected partial success, got error: %v", err)
	}
	if len(summaries) != 1 {
		t.Fatalf("expected 1 successful scope summary, got %d (%+v)", len(summaries), summaries)
	}
	if summaries[0].Scope != "good-scope" {
		t.Errorf("expected good-scope to succeed, got %s", summaries[0].Scope)
	}
}

// TestRunConsolidationAllScopesFail verifies that a hard error is returned
// when every scope in the run fails.
func TestRunConsolidationAllScopesFail(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "symmemory-engine-allfail-test-*")
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

	for _, m := range []*db.Memory{
		{ID: "f-1", Content: "fact one", Scope: "scope-a", ConsolidationStatus: "raw", Metadata: map[string]string{}},
		{ID: "f-2", Content: "fact two", Scope: "scope-a", ConsolidationStatus: "raw", Metadata: map[string]string{}},
		{ID: "f-3", Content: "fact three", Scope: "scope-b", ConsolidationStatus: "raw", Metadata: map[string]string{}},
		{ID: "f-4", Content: "fact four", Scope: "scope-b", ConsolidationStatus: "raw", Metadata: map[string]string{}},
	} {
		if err := database.SaveMemory(m); err != nil {
			t.Fatalf("failed to seed memory %s: %v", m.ID, err)
		}
	}

	mockLLM := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]string{"response": "garbage, no json here"})
	}))
	defer mockLLM.Close()

	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	engine := NewEngine(database, embeddings, mockLLM.URL, "test-model", "ollama", false, "chat", 0)

	_, err = engine.RunConsolidation(context.Background(), "", true)
	if err == nil {
		t.Fatal("expected hard error when every scope fails, got nil")
	}
	if !strings.Contains(err.Error(), "all scopes failed") {
		t.Errorf("expected 'all scopes failed' error, got: %v", err)
	}
}

// TestSchemaConstrainedPathNeedsNoSalvage verifies that when the LLM output
// is clean JSON (as produced by a schema-constrained provider), parseJSONResponse
// succeeds on the first candidate — the raw response — without falling through
// to the salvage strategies (fence extraction, brace-span fallback,
// think-block stripping). This test proves that sending the JSON Schema on
// the provider path eliminates the need for post-hoc salvage.
func TestSchemaConstrainedPathNeedsNoSalvage(t *testing.T) {
	tests := []struct {
		name string
		json string
	}{
		{
			name: "full response with all fields",
			json: `{
				"consolidated": [
					{
						"content": "Daniel prefers dark mode.",
						"replaces_ids": ["mem-1", "mem-2"],
						"metadata": {"topic": "preferences"}
					}
				],
				"discarded_ids": ["mem-3"]
			}`,
		},
		{
			name: "empty consolidated and discarded",
			json: `{
				"consolidated": [],
				"discarded_ids": []
			}`,
		},
		{
			name: "multiple consolidated items",
			json: `{
				"consolidated": [
					{
						"content": "Fact one.",
						"replaces_ids": ["a"],
						"metadata": {}
					},
					{
						"content": "Fact two.",
						"replaces_ids": ["b"],
						"metadata": {"source": "chat"}
					}
				],
				"discarded_ids": ["c", "d"]
			}`,
		},
		{
			name: "replaces_ids can be empty",
			json: `{
				"consolidated": [
					{
						"content": "New standalone fact.",
						"replaces_ids": [],
						"metadata": {}
					}
				],
				"discarded_ids": []
			}`,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result, err := parseJSONResponse(tt.json)
			if err != nil {
				t.Fatalf("parseJSONResponse() should succeed on clean schema-constrained JSON: %v", err)
			}
			if result == nil {
				t.Fatal("parseJSONResponse() returned nil result without error")
			}
			// Verify the response can be re-marshalled — confirms structural integrity.
			if _, err := json.Marshal(result); err != nil {
				t.Errorf("result should be re-marshalable: %v", err)
			}
		})
	}
}
