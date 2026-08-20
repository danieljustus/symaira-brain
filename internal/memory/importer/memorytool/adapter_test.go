package memorytool

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

func TestSessionAdapterName(t *testing.T) {
	adapter := NewSessionAdapter(NewOpenMemoryImporter(), "")
	if got := adapter.Name(); got != "openmemory" {
		t.Errorf("Name() = %q, want openmemory", got)
	}
}

func TestSessionAdapterDiscoversAndImports(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "memorytool-adapter-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	t.Cleanup(func() { os.RemoveAll(tempDir) })

	exportPath := filepath.Join(tempDir, "export.json")
	data := []byte(`{"memories":[{"id":"mem-1","content":"hello world","created_at":"2026-06-22T12:00:00Z"}]}`)
	if err := os.WriteFile(exportPath, data, 0644); err != nil {
		t.Fatalf("failed to write export: %v", err)
	}

	adapter := NewSessionAdapter(NewOpenMemoryImporter(), exportPath)

	sessions, err := adapter.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}
	if len(sessions) != 1 {
		t.Fatalf("expected 1 session, got %d", len(sessions))
	}

	facts, err := adapter.ImportSession(sessions[0])
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}
	if facts[0].Content != "hello world" {
		t.Errorf("content = %q, want hello world", facts[0].Content)
	}
	if facts[0].Source != "openmemory" {
		t.Errorf("source = %q, want openmemory", facts[0].Source)
	}
	if facts[0].SessionID != exportPath {
		t.Errorf("session id = %q, want %q", facts[0].SessionID, exportPath)
	}
}

func TestSessionAdapterFiltersByModificationTime(t *testing.T) {
	tempDir, err := os.MkdirTemp("", "memorytool-adapter-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	t.Cleanup(func() { os.RemoveAll(tempDir) })

	exportPath := filepath.Join(tempDir, "export.json")
	if err := os.WriteFile(exportPath, []byte(`{"memories":[]}`), 0644); err != nil {
		t.Fatalf("failed to write export: %v", err)
	}
	now := time.Now()
	if err := os.Chtimes(exportPath, now, now); err != nil {
		t.Fatalf("failed to set times: %v", err)
	}

	adapter := NewSessionAdapter(NewOpenMemoryImporter(), exportPath)

	sessions, err := adapter.DiscoverSessions(now.Add(time.Hour))
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}
	if len(sessions) != 0 {
		t.Errorf("expected 0 sessions, got %d", len(sessions))
	}
}

func TestSessionAdapterMetadataConversion(t *testing.T) {
	adapter := NewSessionAdapter(NewOpenMemoryImporter(), "")
	if adapter.Category() != "memory-tool" {
		t.Errorf("Category() = %q, want memory-tool", adapter.Category())
	}
	if adapter.PrivacyLevel() != importer.PrivacyConfidential {
		t.Errorf("PrivacyLevel() = %q, want confidential", adapter.PrivacyLevel())
	}
	if !adapter.RequiresPIIGuard() {
		t.Error("RequiresPIIGuard() = false, want true")
	}
}
