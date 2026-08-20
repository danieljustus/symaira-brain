package importer

import (
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
	"github.com/danieljustus/symaira-corekit/evidencekit"
	"os"
)

func helperRegistryDB(t *testing.T) *db.DB {
	t.Helper()
	tempDir, err := os.MkdirTemp("", "symmemory-importer-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	t.Cleanup(func() { os.RemoveAll(tempDir) })

	cfg := config.Defaults()
	cfg.Database.Path = filepath.Join(tempDir, "test.db")

	database, err := db.Open(cfg)
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	t.Cleanup(func() { _ = database.Close() })
	return database
}

func helperRegistry(t *testing.T, database *db.DB) *Registry {
	t.Helper()
	return NewRegistry(database, extractor.NewEmbeddingsGenerator(config.Defaults()), true)
}

// mockTranscriptImporter implements SessionImporter + TranscriptImporter for testing.
type mockTranscriptImporter struct {
	name     string
	facts    []ImportedFact
	sessions []SessionRef
}

func (m *mockTranscriptImporter) Name() string       { return m.name }
func (m *mockTranscriptImporter) IsTranscript() bool { return true }
func (m *mockTranscriptImporter) DiscoverSessions(since time.Time) ([]SessionRef, error) {
	return m.sessions, nil
}
func (m *mockTranscriptImporter) ImportSession(ref SessionRef) ([]ImportedFact, error) {
	return m.facts, nil
}

// mockCuratedImporter implements SessionImporter but NOT TranscriptImporter.
type mockCuratedImporter struct {
	name     string
	facts    []ImportedFact
	sessions []SessionRef
}

func (m *mockCuratedImporter) Name() string { return m.name }
func (m *mockCuratedImporter) DiscoverSessions(since time.Time) ([]SessionRef, error) {
	return m.sessions, nil
}
func (m *mockCuratedImporter) ImportSession(ref SessionRef) ([]ImportedFact, error) {
	return m.facts, nil
}

func TestStoreFactsPIIRedaction(t *testing.T) {
	database := helperRegistryDB(t)
	r := helperRegistry(t, database)

	facts := []ImportedFact{
		{
			Content:   "Contact alice@example.com for API key ghp_abcdefghijklmnopqrstuvwxyz0123456789",
			Source:    "test-importer",
			SessionID: "sess-001",
			Timestamp: time.Now().UTC(),
			Metadata:  map[string]string{"extra": "info"},
		},
	}

	if err := r.storeFacts(facts, &mockCuratedImporter{name: "test-importer"}); err != nil {
		t.Fatalf("storeFacts failed: %v", err)
	}

	memories, err := database.ListMemoriesLite("", 0, 10)
	if err != nil {
		t.Fatalf("ListMemoriesLite failed: %v", err)
	}
	if len(memories) != 1 {
		t.Fatalf("expected 1 memory, got %d", len(memories))
	}

	m := memories[0]
	if strings.Contains(m.Content, "alice@example.com") {
		t.Errorf("expected content email redacted, got %q", m.Content)
	}
	if strings.Contains(m.Content, "ghp_") {
		t.Errorf("expected content API key redacted, got %q", m.Content)
	}
}

func TestStoreFactsMetadataPIIRedaction(t *testing.T) {
	database := helperRegistryDB(t)
	r := helperRegistry(t, database)

	facts := []ImportedFact{
		{
			Content:   "clean content",
			Source:    "test-importer",
			SessionID: "sess-002",
			Timestamp: time.Now().UTC(),
			Metadata:  map[string]string{"contact": "bob@example.com"},
		},
	}

	if err := r.storeFacts(facts, &mockCuratedImporter{name: "test-importer"}); err != nil {
		t.Fatalf("storeFacts failed: %v", err)
	}

	memories, err := database.ListMemoriesLite("", 0, 10)
	if err != nil {
		t.Fatalf("ListMemoriesLite failed: %v", err)
	}
	if len(memories) != 1 {
		t.Fatalf("expected 1 memory, got %d", len(memories))
	}

	m := memories[0]
	if m.Metadata["contact"] != "[REDACTED_EMAIL]" {
		t.Errorf("expected metadata email redacted, got %q", m.Metadata["contact"])
	}
	if m.Metadata["source_type"] != "imported" {
		t.Errorf("expected source_type 'imported', got %q", m.Metadata["source_type"])
	}
	if m.Metadata["source_tool"] != "test-importer" {
		t.Errorf("expected source_tool 'test-importer', got %q", m.Metadata["source_tool"])
	}
}

func TestStoreFactsNilMetadata(t *testing.T) {
	database := helperRegistryDB(t)
	r := helperRegistry(t, database)

	facts := []ImportedFact{
		{
			Content:   "test content",
			Source:    "test-importer",
			SessionID: "sess-003",
			Timestamp: time.Now().UTC(),
			Metadata:  nil,
		},
	}

	if err := r.storeFacts(facts, &mockCuratedImporter{name: "test-importer"}); err != nil {
		t.Fatalf("storeFacts failed: %v", err)
	}

	memories, err := database.ListMemoriesLite("", 0, 10)
	if err != nil {
		t.Fatalf("ListMemoriesLite failed: %v", err)
	}
	if len(memories) != 1 {
		t.Fatalf("expected 1 memory, got %d", len(memories))
	}

	m := memories[0]
	if m.Metadata["source_type"] != "imported" {
		t.Errorf("expected source_type 'imported' from nil metadata, got %q", m.Metadata["source_type"])
	}
	if m.Metadata["source_tool"] != "test-importer" {
		t.Errorf("expected source_tool 'test-importer' from nil metadata, got %q", m.Metadata["source_tool"])
	}
}

func TestStoreFactsGeneratesEmbedding(t *testing.T) {
	database := helperRegistryDB(t)
	r := helperRegistry(t, database)

	facts := []ImportedFact{
		{
			Content:   "Alice prefers dark mode in all applications",
			Source:    "test-importer",
			SessionID: "sess-emb-001",
			Timestamp: time.Now().UTC(),
		},
	}

	if err := r.storeFacts(facts, &mockCuratedImporter{name: "test-importer"}); err != nil {
		t.Fatalf("storeFacts failed: %v", err)
	}

	memories, err := database.ListMemories("", 0, 10)
	if err != nil {
		t.Fatalf("ListMemories failed: %v", err)
	}
	if len(memories) != 1 {
		t.Fatalf("expected 1 memory, got %d", len(memories))
	}

	m := memories[0]
	if len(m.Embedding) == 0 {
		t.Fatal("expected embedding to be generated for imported memory")
	}
	if len(m.Embedding) != 768 {
		t.Errorf("expected 768-dim embedding, got %d", len(m.Embedding))
	}
}

func TestStoreFactsPersistsEvidence(t *testing.T) {
	database := helperRegistryDB(t)
	r := helperRegistry(t, database)

	facts := []ImportedFact{
		{
			Content:   "User prefers dark mode",
			Source:    "test-importer",
			SessionID: "sess-evidence-001",
			Timestamp: time.Now().UTC(),
			Evidence: []evidencekit.Extraction{
				{
					Source:          evidencekit.SourceRef{ID: "sess-evidence-001", Kind: "test-importer"},
					Text:            "User prefers dark mode",
					EvidenceText:    "I always use dark mode",
					Span:            evidencekit.Span{Start: 0, End: 23},
					AlignmentStatus: evidencekit.AlignmentExact,
				},
			},
		},
	}

	if err := r.storeFacts(facts, &mockCuratedImporter{name: "test-importer"}); err != nil {
		t.Fatalf("storeFacts failed: %v", err)
	}

	memories, err := database.ListMemoriesLite("", 0, 10)
	if err != nil {
		t.Fatalf("ListMemoriesLite failed: %v", err)
	}
	if len(memories) != 1 {
		t.Fatalf("expected 1 memory, got %d", len(memories))
	}

	evidence, err := database.GetMemoryEvidence(memories[0].ID)
	if err != nil {
		t.Fatalf("GetMemoryEvidence failed: %v", err)
	}
	if len(evidence) != 1 {
		t.Fatalf("expected 1 evidence row, got %d", len(evidence))
	}
	if evidence[0].SourceID != "sess-evidence-001" {
		t.Errorf("expected source_id 'sess-evidence-001', got %q", evidence[0].SourceID)
	}
}

func TestStoreFactsRedactsEvidenceText(t *testing.T) {
	database := helperRegistryDB(t)
	r := helperRegistry(t, database)

	facts := []ImportedFact{
		{
			Content:   "clean content",
			Source:    "test-importer",
			SessionID: "sess-evidence-002",
			Timestamp: time.Now().UTC(),
			Evidence: []evidencekit.Extraction{
				{
					EvidenceText:    "contact alice@example.com about it",
					Span:            evidencekit.Span{Start: 0, End: 5},
					AlignmentStatus: evidencekit.AlignmentExact,
				},
			},
		},
	}

	if err := r.storeFacts(facts, &mockCuratedImporter{name: "test-importer"}); err != nil {
		t.Fatalf("storeFacts failed: %v", err)
	}

	memories, err := database.ListMemoriesLite("", 0, 10)
	if err != nil {
		t.Fatalf("ListMemoriesLite failed: %v", err)
	}
	evidence, err := database.GetMemoryEvidence(memories[0].ID)
	if err != nil {
		t.Fatalf("GetMemoryEvidence failed: %v", err)
	}
	if len(evidence) != 1 {
		t.Fatalf("expected 1 evidence row, got %d", len(evidence))
	}
	if strings.Contains(evidence[0].EvidenceText, "alice@example.com") {
		t.Errorf("expected evidence text email redacted, got %q", evidence[0].EvidenceText)
	}
}

func TestExtractFactsPropagatesEvidence(t *testing.T) {
	database := helperRegistryDB(t)
	r := helperRegistry(t, database)

	raw := []ImportedFact{
		{Content: "I like dark mode in all my apps.", Source: "claude-code", SessionID: "s-evidence", Timestamp: time.Now().UTC()},
	}

	distilled := r.extractFacts(raw)

	found := false
	for _, f := range distilled {
		if f.Metadata["method"] != "regex_pattern" {
			continue
		}
		found = true
		if len(f.Evidence) != 1 {
			t.Fatalf("expected 1 evidence span on distilled fact, got %d", len(f.Evidence))
		}
		ev := f.Evidence[0]
		if ev.Source.ID != "s-evidence" || ev.Source.Kind != "claude-code" {
			t.Errorf("expected evidence source {s-evidence claude-code}, got %+v", ev.Source)
		}
		if err := evidencekit.Validate(ev); err != nil {
			t.Errorf("expected valid grounded evidence, got: %v", err)
		}
	}
	if !found {
		t.Fatal("expected at least one regex_pattern fact with evidence")
	}
}

func TestRunImportUnknownToolError(t *testing.T) {
	database := helperRegistryDB(t)
	r := helperRegistry(t, database)

	_, err := r.RunImport(t.Context(), []string{"does-not-exist"}, true)
	if err == nil {
		t.Fatal("expected error for unknown importer")
	}
	if !strings.Contains(err.Error(), "does-not-exist") {
		t.Errorf("expected error to name unknown importer, got %q", err.Error())
	}
	if !strings.Contains(err.Error(), "valid importers") {
		t.Errorf("expected error to list valid importers, got %q", err.Error())
	}
}

func TestImportedMemorySearchable(t *testing.T) {
	database := helperRegistryDB(t)
	r := helperRegistry(t, database)

	content := "Alice prefers dark mode in all applications"
	facts := []ImportedFact{
		{
			Content:   content,
			Source:    "test-importer",
			SessionID: "sess-search-001",
			Timestamp: time.Now().UTC(),
		},
	}

	if err := r.storeFacts(facts, &mockCuratedImporter{name: "test-importer"}); err != nil {
		t.Fatalf("storeFacts failed: %v", err)
	}

	memories, err := database.ListMemories("", 0, 10)
	if err != nil {
		t.Fatalf("ListMemories failed: %v", err)
	}
	if len(memories) != 1 {
		t.Fatalf("expected 1 memory, got %d", len(memories))
	}

	m := memories[0]
	if len(m.Embedding) == 0 {
		t.Fatal("expected imported memory to have a non-empty embedding")
	}

	eg := extractor.NewEmbeddingsGenerator(config.Defaults())
	emb := eg.GenerateVector(content)
	queryVec := emb.Vector

	results, err := database.SearchMemories(queryVec, emb.Source, "", 10)
	if err != nil {
		t.Fatalf("SearchMemories failed: %v", err)
	}

	found := false
	for _, r := range results {
		if r.Memory.ID == m.ID {
			found = true
			if len(r.Memory.Embedding) == 0 {
				t.Error("search result should include embedding data")
			}
			break
		}
	}
	if !found {
		t.Errorf("imported memory not found via semantic search (LSH may not match for local hash vectors; embedding is %d-dim)", len(m.Embedding))
	}
}

func TestExtractFactsDistillsTranscript(t *testing.T) {
	database := helperRegistryDB(t)
	r := helperRegistry(t, database)

	raw := []ImportedFact{
		{Content: "I like dark mode in all my apps. My project is symaira-memory. I use TypeScript for frontend.", Source: "claude-code", SessionID: "s1", Timestamp: time.Now().UTC()},
		{Content: "I prefer vim as my editor. We are building an agent memory system with golang.", Source: "claude-code", SessionID: "s1", Timestamp: time.Now().UTC()},
	}

	distilled := r.extractFacts(raw)

	if len(distilled) == 0 {
		t.Fatal("expected at least one distilled fact")
	}

	hasPatternFact := false
	hasSummary := false
	for _, f := range distilled {
		if f.Metadata["method"] == "regex_pattern" {
			hasPatternFact = true
		}
		if f.Metadata["method"] == "extractive_summarization" {
			hasSummary = true
		}
		if f.Source != "claude-code" {
			t.Errorf("expected source 'claude-code', got %q", f.Source)
		}
		if f.SessionID != "s1" {
			t.Errorf("expected session_id 's1', got %q", f.SessionID)
		}
	}

	if !hasPatternFact {
		t.Error("expected at least one regex_pattern fact from extraction")
	}
	if !hasSummary {
		t.Error("expected extractive_summarization summary fact")
	}
}

func TestExtractFactsFallsBackOnEmptyContent(t *testing.T) {
	database := helperRegistryDB(t)
	r := helperRegistry(t, database)

	raw := []ImportedFact{
		{Content: "hello", Source: "test", SessionID: "s2", Timestamp: time.Now().UTC()},
	}

	distilled := r.extractFacts(raw)

	if len(distilled) != 1 {
		t.Fatalf("expected 1 fact (fallback), got %d", len(distilled))
	}
	if distilled[0].Content != "hello" {
		t.Errorf("expected original content preserved, got %q", distilled[0].Content)
	}
}

func TestExtractFactsDisabled(t *testing.T) {
	database := helperRegistryDB(t)
	r := NewRegistry(database, extractor.NewEmbeddingsGenerator(config.Defaults()), false)

	mock := &mockTranscriptImporter{
		name: "test-transcript",
		facts: []ImportedFact{
			{Content: "I like dark mode. My project is symaira-memory.", Source: "test", SessionID: "s3", Timestamp: time.Now().UTC()},
		},
		sessions: []SessionRef{{Tool: "test-transcript", SessionID: "s3"}},
	}
	r.Register(mock)

	results, err := r.RunImport(t.Context(), []string{"test-transcript"}, true)
	if err != nil {
		t.Fatalf("RunImport failed: %v", err)
	}
	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}

	if results[0].Facts != 1 {
		t.Errorf("expected 1 raw fact (extraction disabled), got %d", results[0].Facts)
	}
}

func TestCuratedMemoryBypassesExtraction(t *testing.T) {
	database := helperRegistryDB(t)
	r := helperRegistry(t, database)

	mock := &mockCuratedImporter{
		name: "curated-memory",
		facts: []ImportedFact{
			{Content: "I like dark mode. My project is symaira-memory. I use TypeScript.", Source: "curated-memory", SessionID: "s4", Timestamp: time.Now().UTC()},
		},
		sessions: []SessionRef{{Tool: "curated-memory", SessionID: "s4"}},
	}
	r.Register(mock)

	results, err := r.RunImport(t.Context(), []string{"curated-memory"}, true)
	if err != nil {
		t.Fatalf("RunImport failed: %v", err)
	}
	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}

	if results[0].Facts != 1 {
		t.Errorf("expected 1 fact preserved (curated-memory bypasses extraction), got %d", results[0].Facts)
	}
}

func TestMergeMetadata(t *testing.T) {
	base := map[string]string{"a": "1", "b": "2"}
	extra := map[string]string{"b": "overridden", "c": "3"}

	merged := mergeMetadata(base, extra)

	if merged["a"] != "1" {
		t.Errorf("expected 'a'='1', got %q", merged["a"])
	}
	if merged["b"] != "overridden" {
		t.Errorf("expected 'b'='overridden', got %q", merged["b"])
	}
	if merged["c"] != "3" {
		t.Errorf("expected 'c'='3', got %q", merged["c"])
	}
	if len(merged) != 3 {
		t.Errorf("expected 3 keys, got %d", len(merged))
	}
}

func TestExtractFactsPreservesTranscriptInterface(t *testing.T) {
	var _ TranscriptImporter = (*mockTranscriptImporter)(nil)
}

func TestNonTranscriptImporterNotExtracted(t *testing.T) {
	database := helperRegistryDB(t)
	r := helperRegistry(t, database)

	mock := &mockCuratedImporter{
		name: "notes",
		facts: []ImportedFact{
			{Content: "I like dark mode. My project is symaira-memory.", Source: "notes", SessionID: "s5", Timestamp: time.Now().UTC()},
		},
		sessions: []SessionRef{{Tool: "notes", SessionID: "s5"}},
	}
	r.Register(mock)

	results, err := r.RunImport(t.Context(), []string{"notes"}, true)
	if err != nil {
		t.Fatalf("RunImport failed: %v", err)
	}
	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}

	if results[0].Facts != 1 {
		t.Errorf("expected 1 fact (non-transcript bypasses extraction), got %d", results[0].Facts)
	}
}
