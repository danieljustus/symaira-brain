package memorytool

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestNewMem0Importer(t *testing.T) {
	imp := NewMem0Importer()
	if imp.Name() != "mem0" {
		t.Errorf("Name() = %q, want %q", imp.Name(), "mem0")
	}
}

func TestMem0DiscoverExports_FileDirect(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "export.json")
	if err := os.WriteFile(path, []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewMem0Importer()
	refs, err := imp.DiscoverExports(path)
	if err != nil {
		t.Fatalf("DiscoverExports failed: %v", err)
	}
	if len(refs) != 1 {
		t.Fatalf("expected 1 ref, got %d", len(refs))
	}
	if refs[0].Tool != "mem0" {
		t.Errorf("Tool = %q, want %q", refs[0].Tool, "mem0")
	}
}

func TestMem0DiscoverExports_NonJsonFile(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "data.txt")
	if err := os.WriteFile(path, []byte("test"), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewMem0Importer()
	refs, err := imp.DiscoverExports(path)
	if err != nil {
		t.Fatalf("DiscoverExports failed: %v", err)
	}
	if len(refs) != 0 {
		t.Errorf("expected 0 refs for non-JSON file, got %d", len(refs))
	}
}

func TestMem0DiscoverExports_DirectoryWalk(t *testing.T) {
	tmpDir := t.TempDir()
	subDir := filepath.Join(tmpDir, "sub")
	os.MkdirAll(subDir, 0o755)
	os.WriteFile(filepath.Join(tmpDir, "memories.json"), []byte("{}"), 0o644)
	os.WriteFile(filepath.Join(subDir, "backup.json"), []byte("{}"), 0o644)

	imp := NewMem0Importer()
	refs, err := imp.DiscoverExports(tmpDir)
	if err != nil {
		t.Fatalf("DiscoverExports failed: %v", err)
	}
	if len(refs) != 2 {
		t.Fatalf("expected 2 refs from directory walk, got %d", len(refs))
	}
}

func TestMem0DiscoverExports_EmptyPathDefaults(t *testing.T) {
	imp := NewMem0Importer()
	_, err := imp.DiscoverExports("")
	if err == nil {
		t.Log("empty path default resolved without error (expected in dev env)")
	}
}

func TestMem0DiscoverExports_MissingPath(t *testing.T) {
	imp := NewMem0Importer()
	_, err := imp.DiscoverExports("/nonexistent/path")
	if err == nil {
		t.Error("expected error for nonexistent path, got nil")
	}
}

func TestMem0ImportExport_Valid(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "export.json")
	data := `{
		"memories": [
			{
				"id": "mem1",
				"content": "Test memory content",
				"user_id": "user123",
				"agent_id": "agent456",
				"created_at": "2026-01-01T12:00:00Z"
			}
		]
	}`
	if err := os.WriteFile(path, []byte(data), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewMem0Importer()
	ref := ExportRef{Tool: "mem0", Path: path, Format: "json", ModifiedAt: time.Now()}
	facts, err := imp.ImportExport(ref)
	if err != nil {
		t.Fatalf("ImportExport failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}
	if facts[0].Source != "mem0" {
		t.Errorf("Source = %q, want %q", facts[0].Source, "mem0")
	}
	if facts[0].Content != "Test memory content" {
		t.Errorf("Content = %q, want %q", facts[0].Content, "Test memory content")
	}
	if facts[0].Metadata["mem0_id"] != "mem1" {
		t.Errorf("mem0_id = %v, want %q", facts[0].Metadata["mem0_id"], "mem1")
	}
	if facts[0].Metadata["user_id"] != "user123" {
		t.Errorf("user_id = %v, want %q", facts[0].Metadata["user_id"], "user123")
	}
	if facts[0].Metadata["agent_id"] != "agent456" {
		t.Errorf("agent_id = %v, want %q", facts[0].Metadata["agent_id"], "agent456")
	}
}

func TestMem0ImportExport_EmptyUserIDAgentID(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "export.json")
	data := `{
		"memories": [
			{
				"id": "mem1",
				"content": "No user/agent",
				"user_id": "",
				"agent_id": "",
				"created_at": "2026-01-01T12:00:00Z"
			}
		]
	}`
	if err := os.WriteFile(path, []byte(data), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewMem0Importer()
	ref := ExportRef{Tool: "mem0", Path: path, Format: "json"}
	facts, err := imp.ImportExport(ref)
	if err != nil {
		t.Fatalf("ImportExport failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}
	// Empty user_id and agent_id should NOT appear in metadata.
	if _, ok := facts[0].Metadata["user_id"]; ok {
		t.Error("unexpected user_id in metadata for empty value")
	}
	if _, ok := facts[0].Metadata["agent_id"]; ok {
		t.Error("unexpected agent_id in metadata for empty value")
	}
}

func TestMem0ImportExport_MultipleMemories(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "export.json")
	data := `{
		"memories": [
			{"id": "m1", "content": "First memory", "user_id": "u1", "agent_id": "", "created_at": "2026-01-01T12:00:00Z"},
			{"id": "m2", "content": "Second memory", "user_id": "", "agent_id": "a1", "created_at": "2026-01-02T12:00:00Z"}
		]
	}`
	if err := os.WriteFile(path, []byte(data), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewMem0Importer()
	ref := ExportRef{Tool: "mem0", Path: path, Format: "json"}
	facts, err := imp.ImportExport(ref)
	if err != nil {
		t.Fatalf("ImportExport failed: %v", err)
	}
	if len(facts) != 2 {
		t.Fatalf("expected 2 facts, got %d", len(facts))
	}
	if facts[0].Metadata["user_id"] != "u1" {
		t.Errorf("fact[0] user_id = %v, want %q", facts[0].Metadata["user_id"], "u1")
	}
	if _, ok := facts[1].Metadata["user_id"]; ok {
		t.Error("fact[1] should not have user_id (empty)")
	}
	if facts[1].Metadata["agent_id"] != "a1" {
		t.Errorf("fact[1] agent_id = %v, want %q", facts[1].Metadata["agent_id"], "a1")
	}
}

func TestMem0ImportExport_EmptyMemories(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "export.json")
	data := `{"memories": []}`
	if err := os.WriteFile(path, []byte(data), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewMem0Importer()
	ref := ExportRef{Tool: "mem0", Path: path, Format: "json"}
	facts, err := imp.ImportExport(ref)
	if err != nil {
		t.Fatalf("ImportExport failed: %v", err)
	}
	if len(facts) != 0 {
		t.Errorf("expected 0 facts for empty memories, got %d", len(facts))
	}
}

func TestMem0ImportExport_InvalidJSON(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "export.json")
	if err := os.WriteFile(path, []byte("{bad json"), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewMem0Importer()
	ref := ExportRef{Tool: "mem0", Path: path, Format: "json"}
	_, err := imp.ImportExport(ref)
	if err == nil {
		t.Error("expected error for invalid JSON, got nil")
	}
	if err != nil && !strings.Contains(err.Error(), "failed to parse JSON") {
		t.Errorf("unexpected error: %v", err)
	}
}

func TestMem0ImportExport_MissingFile(t *testing.T) {
	imp := NewMem0Importer()
	ref := ExportRef{Tool: "mem0", Path: "/nonexistent/file.json", Format: "json"}
	_, err := imp.ImportExport(ref)
	if err == nil {
		t.Error("expected error for missing file, got nil")
	}
}

func TestMem0ImportExport_InvalidDate(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "export.json")
	data := `{"memories": [{"id": "m1", "content": "test", "user_id": "", "agent_id": "", "created_at": "not-a-date"}]}`
	if err := os.WriteFile(path, []byte(data), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewMem0Importer()
	ref := ExportRef{Tool: "mem0", Path: path, Format: "json"}
	facts, err := imp.ImportExport(ref)
	if err != nil {
		t.Fatalf("ImportExport failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}
	// Invalid date should result in zero timestamp, not an error.
	if !facts[0].Timestamp.IsZero() {
		t.Errorf("expected zero timestamp for invalid date, got %v", facts[0].Timestamp)
	}
}
