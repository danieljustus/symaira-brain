package memory

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/extractor"
)

func TestPrepareEmptyScopeDefaultsToGlobal(t *testing.T) {
	mem, _, err := Prepare("test content", "", nil, false, Attribution{}, "test")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if mem.Scope != "global" {
		t.Errorf("expected scope 'global', got '%s'", mem.Scope)
	}
}

func TestPrepareInvalidScopeReturnsError(t *testing.T) {
	mem, _, err := Prepare("test content", "banana", nil, false, Attribution{}, "test")
	if err == nil {
		t.Fatal("expected error for invalid scope, got nil")
	}
	if mem != nil {
		t.Errorf("expected nil memory on error, got %+v", mem)
	}
}

func TestPrepareStampsTrustDefaults(t *testing.T) {
	mem, _, err := Prepare("test content", "global", nil, false, Attribution{}, "test")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if mem.Metadata[MetaAuthority] != AuthorityDirect {
		t.Errorf("expected authority=%q, got %q", AuthorityDirect, mem.Metadata[MetaAuthority])
	}
	if mem.Metadata[MetaConfidence] != ConfidenceHigh {
		t.Errorf("expected confidence=%q, got %q", ConfidenceHigh, mem.Metadata[MetaConfidence])
	}
	if mem.Metadata[MetaVerificationStatus] != VerificationUnverified {
		t.Errorf("expected verification_status=%q, got %q", VerificationUnverified, mem.Metadata[MetaVerificationStatus])
	}
}

func TestPrepareStampsPolicyDefaults(t *testing.T) {
	mem, _, err := Prepare("test content", "global", nil, false, Attribution{}, "test")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if mem.Metadata[MetaSensitivity] != SensitivityInternal {
		t.Errorf("expected sensitivity=%q, got %q", SensitivityInternal, mem.Metadata[MetaSensitivity])
	}
	if mem.Metadata[MetaSharingLevel] != SharingPrivate {
		t.Errorf("expected sharing_level=%q, got %q", SharingPrivate, mem.Metadata[MetaSharingLevel])
	}
}

func TestPreparePreservesExplicitMetadata(t *testing.T) {
	meta := map[string]string{
		MetaSensitivity:  SensitivityConfidential,
		MetaSharingLevel: SharingTeam,
		MetaAuthority:    AuthorityVerified,
		MetaConfidence:   ConfidenceLow,
		"custom_key":     "custom_value",
	}
	mem, _, err := Prepare("test content", "global", meta, false, Attribution{}, "test")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if mem.Metadata[MetaSensitivity] != SensitivityConfidential {
		t.Errorf("expected explicit sensitivity preserved, got %q", mem.Metadata[MetaSensitivity])
	}
	if mem.Metadata[MetaSharingLevel] != SharingTeam {
		t.Errorf("expected explicit sharing_level preserved, got %q", mem.Metadata[MetaSharingLevel])
	}
	if mem.Metadata[MetaAuthority] != AuthorityVerified {
		t.Errorf("expected explicit authority preserved, got %q", mem.Metadata[MetaAuthority])
	}
	if mem.Metadata["custom_key"] != "custom_value" {
		t.Errorf("expected custom metadata preserved, got %q", mem.Metadata["custom_key"])
	}
}

func TestPreparePIIRedaction(t *testing.T) {
	content := "contact me at john@example.com for details"
	mem, _, err := Prepare(content, "global", nil, true, Attribution{}, "test")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if strings.Contains(mem.Content, "john@example.com") {
		t.Errorf("expected PII to be redacted, but content still contains email: %s", mem.Content)
	}
}

func TestPreparePIIMetadataRedaction(t *testing.T) {
	meta := map[string]string{
		"source":  "import",
		"contact": "alice@example.com",
		"token":   "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
	}
	mem, _, err := Prepare("clean content", "global", meta, true, Attribution{}, "test")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if mem.Metadata["contact"] != "[REDACTED_EMAIL]" {
		t.Errorf("expected metadata email redacted, got %q", mem.Metadata["contact"])
	}
	if mem.Metadata["token"] != "[REDACTED_API_KEY]" {
		t.Errorf("expected metadata API key redacted, got %q", mem.Metadata["token"])
	}
	if mem.Metadata["source"] != "import" {
		t.Errorf("expected clean metadata preserved, got %q", mem.Metadata["source"])
	}
}

func TestPreparePIIMetadataDisabled(t *testing.T) {
	meta := map[string]string{"contact": "alice@example.com"}
	mem, _, err := Prepare("clean content", "global", meta, false, Attribution{}, "test")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if mem.Metadata["contact"] != "alice@example.com" {
		t.Errorf("expected metadata unchanged when PII disabled, got %q", mem.Metadata["contact"])
	}
}

func TestPreparePIIDisabled(t *testing.T) {
	content := "contact me at john@example.com for details"
	mem, _, err := Prepare(content, "global", nil, false, Attribution{}, "test")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if mem.Content != content {
		t.Errorf("expected content unchanged when PII disabled, got '%s'", mem.Content)
	}
}

func TestPrepareNilMetaBecomesEmptyMap(t *testing.T) {
	mem, _, err := Prepare("test content", "global", nil, false, Attribution{}, "test")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if mem.Metadata == nil {
		t.Error("expected non-nil metadata map, got nil")
	}
	if len(mem.Metadata) != 9 {
		t.Errorf("expected 9 metadata entries (4 provenance + 3 trust + 2 policy), got %d", len(mem.Metadata))
	}
}

func TestPrepareProjectScopeSetsProjectName(t *testing.T) {
	// Create temp dir with .git directory to simulate project root
	tempDir, err := os.MkdirTemp("", "symmemory-memory-test-*")
	if err != nil {
		t.Fatalf("failed to create temp directory: %v", err)
	}
	defer os.RemoveAll(tempDir)

	gitDir := filepath.Join(tempDir, ".git")
	if err := os.MkdirAll(gitDir, 0755); err != nil {
		t.Fatalf("failed to create .git directory: %v", err)
	}

	// Save original cwd and switch to temp dir
	oldCwd, err := os.Getwd()
	if err != nil {
		t.Fatalf("failed to get current working directory: %v", err)
	}
	if err := os.Chdir(tempDir); err != nil {
		t.Fatalf("failed to change to temp directory: %v", err)
	}
	defer os.Chdir(oldCwd)

	mem, _, err := Prepare("test content", "project", nil, false, Attribution{}, "test")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if mem.Metadata["project_name"] == "" {
		t.Error("expected project_name to be set in metadata for project scope")
	}
}

func TestPrepareAttribution(t *testing.T) {
	attr := Attribution{Author: "cli:daniel", SessionID: "sess-123"}
	mem, _, err := Prepare("test content", "global", nil, false, attr, "test")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if mem.CreatedBy != "cli:daniel" {
		t.Errorf("expected CreatedBy 'cli:daniel', got %q", mem.CreatedBy)
	}
	if mem.UpdatedBy != "cli:daniel" {
		t.Errorf("expected UpdatedBy 'cli:daniel', got %q", mem.UpdatedBy)
	}
	if mem.CreatedSession != "sess-123" {
		t.Errorf("expected CreatedSession 'sess-123', got %q", mem.CreatedSession)
	}
	if mem.UpdatedSession != "sess-123" {
		t.Errorf("expected UpdatedSession 'sess-123', got %q", mem.UpdatedSession)
	}
}

func helperMemDB(t *testing.T) *db.DB {
	t.Helper()
	tempDir, err := os.MkdirTemp("", "symmemory-memory-test-*")
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

func TestStoreWithEntityLinking(t *testing.T) {
	database := helperMemDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	patternExtractor := extractor.NewPatternExtractor()

	attr := Attribution{Author: "test-user", SessionID: "sess-1"}
	entities := []string{"Alice", "Bob"}

	m, extractedStr, err := Store(database, embeddings, patternExtractor, "Alice and Bob discussed the project", "global", nil, false, attr, entities, "test", false, 0)
	if err != nil {
		t.Fatalf("Store failed: %v", err)
	}

	if m.ID == "" {
		t.Error("expected non-empty memory ID")
	}
	if m.Content != "Alice and Bob discussed the project" {
		t.Errorf("expected original content, got '%s'", m.Content)
	}

	got, err := database.GetMemory(m.ID)
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got == nil {
		t.Fatal("expected memory to be saved")
	}
	if got.CreatedBy != "test-user" {
		t.Errorf("expected CreatedBy 'test-user', got '%s'", got.CreatedBy)
	}

	alice, err := database.ResolveEntity("Alice")
	if err != nil {
		t.Fatalf("ResolveEntity failed: %v", err)
	}
	if alice == nil {
		t.Fatal("expected entity 'Alice' to be created")
	}

	bob, err := database.ResolveEntity("Bob")
	if err != nil {
		t.Fatalf("ResolveEntity failed: %v", err)
	}
	if bob == nil {
		t.Fatal("expected entity 'Bob' to be created")
	}

	_ = extractedStr
}

func TestStoreCreatesEntityIfNew(t *testing.T) {
	database := helperMemDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	patternExtractor := extractor.NewPatternExtractor()

	attr := Attribution{Author: "test"}
	entities := []string{"NewEntity"}

	m, _, err := Store(database, embeddings, patternExtractor, "Test with new entity", "global", nil, false, attr, entities, "test", false, 0)
	if err != nil {
		t.Fatalf("Store failed: %v", err)
	}

	entity, err := database.ResolveEntity("NewEntity")
	if err != nil {
		t.Fatalf("ResolveEntity failed: %v", err)
	}
	if entity == nil {
		t.Fatal("expected entity 'NewEntity' to be auto-created")
	}
	if entity.CreatedBy != "test" {
		t.Errorf("expected entity CreatedBy 'test', got '%s'", entity.CreatedBy)
	}

	got, _ := database.GetMemory(m.ID)
	if got == nil {
		t.Fatal("expected memory to be saved")
	}
	if len(got.Entities) != 1 || got.Entities[0] != "NewEntity" {
		t.Errorf("expected memory to have entity 'NewEntity', got %v", got.Entities)
	}
}

func TestStoreSkipsEmptyEntityNames(t *testing.T) {
	database := helperMemDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	patternExtractor := extractor.NewPatternExtractor()

	attr := Attribution{Author: "test"}
	entities := []string{"", "  ", "ValidEntity"}

	_, _, err := Store(database, embeddings, patternExtractor, "Test entity skipping", "global", nil, false, attr, entities, "test", false, 0)
	if err != nil {
		t.Fatalf("Store failed: %v", err)
	}

	entity, err := database.ResolveEntity("ValidEntity")
	if err != nil {
		t.Fatalf("ResolveEntity failed: %v", err)
	}
	if entity == nil {
		t.Fatal("expected 'ValidEntity' to exist")
	}
}

func TestStoreWithPIIRedaction(t *testing.T) {
	database := helperMemDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	patternExtractor := extractor.NewPatternExtractor()

	attr := Attribution{Author: "test"}

	m, _, err := Store(database, embeddings, patternExtractor, "Contact alice@example.com for info", "global", nil, true, attr, nil, "test", false, 0)
	if err != nil {
		t.Fatalf("Store failed: %v", err)
	}

	got, _ := database.GetMemory(m.ID)
	if got == nil {
		t.Fatal("expected memory to be saved")
	}
	if strings.Contains(got.Content, "alice@example.com") {
		t.Errorf("expected PII to be redacted in stored content, got '%s'", got.Content)
	}
}

func TestStoreWithPIIMetadataRedaction(t *testing.T) {
	database := helperMemDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	patternExtractor := extractor.NewPatternExtractor()

	attr := Attribution{Author: "test"}
	meta := map[string]string{
		"source":  "import",
		"contact": "bob@example.com",
	}

	m, _, err := Store(database, embeddings, patternExtractor, "clean content", "global", meta, true, attr, nil, "test", false, 0)
	if err != nil {
		t.Fatalf("Store failed: %v", err)
	}

	got, _ := database.GetMemory(m.ID)
	if got == nil {
		t.Fatal("expected memory to be saved")
	}
	if got.Metadata["contact"] != "[REDACTED_EMAIL]" {
		t.Errorf("expected metadata email redacted, got %q", got.Metadata["contact"])
	}
	if got.Metadata["source"] != "import" {
		t.Errorf("expected clean metadata preserved, got %q", got.Metadata["source"])
	}
}

func TestStoreDeduplicatesSecondaryFacts(t *testing.T) {
	database := helperMemDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	patternExtractor := extractor.NewPatternExtractor()

	attr := Attribution{Author: "test"}
	content := "I prefer Go for backend"

	m, extractedStr, err := Store(database, embeddings, patternExtractor, content, "global", nil, false, attr, nil, "test", false, 0)
	if err != nil {
		t.Fatalf("Store failed: %v", err)
	}

	if m.Content != content {
		t.Errorf("expected primary content %q, got %q", content, m.Content)
	}
	if len(extractedStr) != 0 {
		t.Errorf("expected no secondary facts when fact duplicates primary, got %v", extractedStr)
	}

	mems, err := database.ListMemoriesLite("", 0, 100)
	if err != nil {
		t.Fatalf("ListMemoriesLite failed: %v", err)
	}
	if len(mems) != 1 {
		t.Errorf("expected 1 memory (primary only), got %d", len(mems))
	}
}

func TestFormatStoreSuccess(t *testing.T) {
	m := &db.Memory{
		ID:       "test-id",
		Content:  "Test content",
		Scope:    "global",
		Metadata: map[string]string{},
	}
	msg := FormatStoreSuccess(m, nil)
	if !strings.Contains(msg, "test-id") {
		t.Errorf("expected message to contain ID, got '%s'", msg)
	}
	if !strings.Contains(msg, "Test content") {
		t.Errorf("expected message to contain content, got '%s'", msg)
	}

	m.Scope = "project"
	m.Metadata["project_name"] = "my-project"
	msg = FormatStoreSuccess(m, nil)
	if !strings.Contains(msg, "my-project") {
		t.Errorf("expected message to contain project name for project scope, got '%s'", msg)
	}

	extracted := []string{"  - [Fact Extracted] fact1 (ID: id1)"}
	msg = FormatStoreSuccess(m, extracted)
	if !strings.Contains(msg, "secondary facts") {
		t.Errorf("expected message to mention secondary facts, got '%s'", msg)
	}
	if !strings.Contains(msg, "fact1") {
		t.Errorf("expected message to contain extracted fact, got '%s'", msg)
	}
}

func TestStore_WorkingTier(t *testing.T) {
	database := helperMemDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	patternExtractor := extractor.NewPatternExtractor()

	attr := Attribution{Author: "test-user", SessionID: "sess-1"}

	m, _, err := Store(database, embeddings, patternExtractor, "Current task: implement feature X", "project", nil, false, attr, nil, "test", true, 24*time.Hour)
	if err != nil {
		t.Fatalf("Store failed: %v", err)
	}

	if m.Tier != "working" {
		t.Errorf("expected Tier=working, got %q", m.Tier)
	}
	if m.ExpiresAt == nil {
		t.Fatal("expected ExpiresAt to be set")
	}

	got, err := database.GetMemory(m.ID)
	if err != nil {
		t.Fatalf("GetMemory failed: %v", err)
	}
	if got == nil {
		t.Fatal("expected memory to be saved")
	}
	if got.Tier != "working" {
		t.Errorf("expected persisted Tier=working, got %q", got.Tier)
	}
}

func TestStore_DefaultTier(t *testing.T) {
	database := helperMemDB(t)
	cfg := config.Defaults()
	embeddings := extractor.NewEmbeddingsGenerator(cfg)
	patternExtractor := extractor.NewPatternExtractor()

	attr := Attribution{Author: "test-user", SessionID: "sess-1"}

	m, _, err := Store(database, embeddings, patternExtractor, "Alice prefers dark mode", "global", nil, false, attr, nil, "test", false, 0)
	if err != nil {
		t.Fatalf("Store failed: %v", err)
	}

	if m.Tier != "long_term" {
		t.Errorf("expected Tier=long_term, got %q", m.Tier)
	}
	if m.ExpiresAt != nil {
		t.Errorf("expected ExpiresAt=nil for long_term, got %v", *m.ExpiresAt)
	}
}
