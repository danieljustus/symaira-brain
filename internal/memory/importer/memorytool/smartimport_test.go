package memorytool

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
)

func helperSmartImportDB(t *testing.T) *db.DB {
	t.Helper()
	tempDir, err := os.MkdirTemp("", "symmemory-smartimport-test-*")
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

func TestStoreFactPIIRedaction(t *testing.T) {
	database := helperSmartImportDB(t)
	si := NewSmartImporter(database)

	facts := []ImportedFact{
		{
			Content:   "User email: carol@example.com, API token: sk-proj-abc123def456ghi789jkl012mno345pqr",
			Source:    "chatgpt",
			Timestamp: time.Now().UTC(),
			Metadata:  map[string]interface{}{"tool": "chatgpt"},
		},
	}

	result, err := si.ImportFacts(facts, false)
	if err != nil {
		t.Fatalf("ImportFacts failed: %v", err)
	}
	if result.Created != 1 {
		t.Fatalf("expected 1 created, got %d", result.Created)
	}

	memories, err := database.ListMemoriesLite("", 0, 10)
	if err != nil {
		t.Fatalf("ListMemoriesLite failed: %v", err)
	}
	if len(memories) != 1 {
		t.Fatalf("expected 1 memory, got %d", len(memories))
	}

	m := memories[0]
	if strings.Contains(m.Content, "carol@example.com") {
		t.Errorf("expected content email redacted, got %q", m.Content)
	}
	if strings.Contains(m.Content, "sk-proj-") {
		t.Errorf("expected content API key redacted, got %q", m.Content)
	}
}

func TestStoreFactMetadataPIIRedaction(t *testing.T) {
	database := helperSmartImportDB(t)
	si := NewSmartImporter(database)

	facts := []ImportedFact{
		{
			Content:   "clean content",
			Source:    "mem0",
			Timestamp: time.Now().UTC(),
			Metadata:  map[string]interface{}{"contact": "dave@example.com"},
		},
	}

	result, err := si.ImportFacts(facts, false)
	if err != nil {
		t.Fatalf("ImportFacts failed: %v", err)
	}
	if result.Created != 1 {
		t.Fatalf("expected 1 created, got %d", result.Created)
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
}

func TestStoreFactDryRunNoPII(t *testing.T) {
	database := helperSmartImportDB(t)
	si := NewSmartImporter(database)

	facts := []ImportedFact{
		{
			Content:   "Contact eve@example.com",
			Source:    "openmemory",
			Timestamp: time.Now().UTC(),
			Metadata:  nil,
		},
	}

	result, err := si.ImportFacts(facts, true)
	if err != nil {
		t.Fatalf("ImportFacts dry run failed: %v", err)
	}
	if result.Created != 1 {
		t.Fatalf("expected 1 created in dry run, got %d", result.Created)
	}

	memories, err := database.ListMemoriesLite("", 0, 10)
	if err != nil {
		t.Fatalf("ListMemoriesLite failed: %v", err)
	}
	if len(memories) != 0 {
		t.Errorf("expected 0 memories in dry run, got %d", len(memories))
	}
}
