package memorytool

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestNewOpenMemoryImporter(t *testing.T) {
	imp := NewOpenMemoryImporter()
	if imp.Name() != "openmemory" {
		t.Errorf("Name() = %q, want %q", imp.Name(), "openmemory")
	}
}

func TestOpenMemoryDiscoverExports_FileDirect(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "export.json")
	if err := os.WriteFile(path, []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewOpenMemoryImporter()
	refs, err := imp.DiscoverExports(path)
	if err != nil {
		t.Fatalf("DiscoverExports failed: %v", err)
	}
	if len(refs) != 1 {
		t.Fatalf("expected 1 ref, got %d", len(refs))
	}
	if refs[0].Tool != "openmemory" {
		t.Errorf("Tool = %q, want %q", refs[0].Tool, "openmemory")
	}
}

func TestOpenMemoryDiscoverExports_NonJsonFile(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "data.txt")
	os.WriteFile(path, []byte("test"), 0o644)

	imp := NewOpenMemoryImporter()
	refs, err := imp.DiscoverExports(path)
	if err != nil {
		t.Fatalf("DiscoverExports failed: %v", err)
	}
	if len(refs) != 0 {
		t.Errorf("expected 0 refs for non-JSON file, got %d", len(refs))
	}
}

func TestOpenMemoryDiscoverExports_MissingPath(t *testing.T) {
	imp := NewOpenMemoryImporter()
	_, err := imp.DiscoverExports("/nonexistent")
	if err == nil {
		t.Error("expected error for missing path, got nil")
	}
}

func TestOpenMemoryDiscoverExports_DirectoryWalk(t *testing.T) {
	tmpDir := t.TempDir()
	os.MkdirAll(filepath.Join(tmpDir, "sub"), 0o755)
	os.WriteFile(filepath.Join(tmpDir, "data.json"), []byte("{}"), 0o644)
	os.WriteFile(filepath.Join(tmpDir, "sub", "more.json"), []byte("{}"), 0o644)

	imp := NewOpenMemoryImporter()
	refs, err := imp.DiscoverExports(tmpDir)
	if err != nil {
		t.Fatalf("DiscoverExports failed: %v", err)
	}
	if len(refs) != 2 {
		t.Fatalf("expected 2 refs from directory walk, got %d", len(refs))
	}
}

func TestOpenMemoryImportExport_Memories(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "export.json")
	data := `{
		"memories": [
			{"id": "m1", "content": "Memory one", "metadata": {"key": "val"}, "created_at": "2026-01-01T12:00:00Z"}
		],
		"facts": []
	}`
	os.WriteFile(path, []byte(data), 0o644)

	imp := NewOpenMemoryImporter()
	ref := ExportRef{Tool: "openmemory", Path: path, Format: "json"}
	facts, err := imp.ImportExport(ref)
	if err != nil {
		t.Fatalf("ImportExport failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}
	if facts[0].Source != "openmemory" {
		t.Errorf("Source = %q, want %q", facts[0].Source, "openmemory")
	}
	if facts[0].Content != "Memory one" {
		t.Errorf("Content = %q, want %q", facts[0].Content, "Memory one")
	}
}

func TestOpenMemoryImportExport_Facts(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "export.json")
	data := `{
		"memories": [],
		"facts": [
			{"subject": "Alice", "predicate": "works-with", "object": "Bob", "confidence": 0.95}
		]
	}`
	os.WriteFile(path, []byte(data), 0o644)

	imp := NewOpenMemoryImporter()
	ref := ExportRef{Tool: "openmemory", Path: path, Format: "json"}
	facts, err := imp.ImportExport(ref)
	if err != nil {
		t.Fatalf("ImportExport failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}
	if facts[0].Content != "Alice works-with Bob" {
		t.Errorf("Content = %q, want %q", facts[0].Content, "Alice works-with Bob")
	}
	if facts[0].Metadata["confidence"] != 0.95 {
		t.Errorf("confidence = %v, want 0.95", facts[0].Metadata["confidence"])
	}
}

func TestOpenMemoryImportExport_Mixed(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "export.json")
	data := `{
		"memories": [
			{"id": "m1", "content": "A memory", "metadata": {}, "created_at": "2026-01-01T12:00:00Z"}
		],
		"facts": [
			{"subject": "X", "predicate": "related-to", "object": "Y", "confidence": 0.8}
		]
	}`
	os.WriteFile(path, []byte(data), 0o644)

	imp := NewOpenMemoryImporter()
	ref := ExportRef{Tool: "openmemory", Path: path, Format: "json"}
	facts, err := imp.ImportExport(ref)
	if err != nil {
		t.Fatalf("ImportExport failed: %v", err)
	}
	if len(facts) != 2 {
		t.Fatalf("expected 2 facts (1 memory + 1 fact), got %d", len(facts))
	}
}

func TestOpenMemoryImportExport_InvalidJSON(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "export.json")
	os.WriteFile(path, []byte("{bad"), 0o644)

	imp := NewOpenMemoryImporter()
	ref := ExportRef{Tool: "openmemory", Path: path, Format: "json"}
	_, err := imp.ImportExport(ref)
	if err == nil {
		t.Error("expected error for invalid JSON, got nil")
	}
	if err != nil && !strings.Contains(err.Error(), "failed to parse JSON") {
		t.Errorf("unexpected error: %v", err)
	}
}

func TestOpenMemoryImportExport_MissingFile(t *testing.T) {
	imp := NewOpenMemoryImporter()
	ref := ExportRef{Tool: "openmemory", Path: "/nonexistent/file.json", Format: "json"}
	_, err := imp.ImportExport(ref)
	if err == nil {
		t.Error("expected error for missing file, got nil")
	}
}

func TestOpenMemoryImportExport_EmptyExport(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "export.json")
	os.WriteFile(path, []byte(`{"memories": [], "facts": []}`), 0o644)

	imp := NewOpenMemoryImporter()
	ref := ExportRef{Tool: "openmemory", Path: path, Format: "json"}
	facts, err := imp.ImportExport(ref)
	if err != nil {
		t.Fatalf("ImportExport failed: %v", err)
	}
	if len(facts) != 0 {
		t.Errorf("expected 0 facts for empty export, got %d", len(facts))
	}
}
