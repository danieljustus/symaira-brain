package memorytool

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestNewChatGPTImporter(t *testing.T) {
	imp := NewChatGPTImporter()
	if imp.Name() != "chatgpt" {
		t.Errorf("Name() = %q, want %q", imp.Name(), "chatgpt")
	}
}

func TestChatGPTDiscoverExports_FileDirect(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "conversations.json")
	if err := os.WriteFile(path, []byte("[]"), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewChatGPTImporter()
	refs, err := imp.DiscoverExports(path)
	if err != nil {
		t.Fatalf("DiscoverExports failed: %v", err)
	}
	if len(refs) != 1 {
		t.Fatalf("expected 1 ref, got %d", len(refs))
	}
	if refs[0].Tool != "chatgpt" {
		t.Errorf("Tool = %q, want %q", refs[0].Tool, "chatgpt")
	}
	if refs[0].Format != "json" {
		t.Errorf("Format = %q, want %q", refs[0].Format, "json")
	}
}

func TestChatGPTDiscoverExports_NonJsonFile(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "data.txt")
	if err := os.WriteFile(path, []byte("test"), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewChatGPTImporter()
	refs, err := imp.DiscoverExports(path)
	if err != nil {
		t.Fatalf("DiscoverExports failed: %v", err)
	}
	if len(refs) != 0 {
		t.Errorf("expected 0 refs for non-JSON file, got %d", len(refs))
	}
}

func TestChatGPTDiscoverExports_DirectoryWalk(t *testing.T) {
	tmpDir := t.TempDir()
	subDir := filepath.Join(tmpDir, "sub")
	if err := os.MkdirAll(subDir, 0o755); err != nil {
		t.Fatal(err)
	}
	p1 := filepath.Join(tmpDir, "conversations.json")
	p2 := filepath.Join(subDir, "conversations.json")
	if err := os.WriteFile(p1, []byte("[]"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(p2, []byte("[]"), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewChatGPTImporter()
	refs, err := imp.DiscoverExports(tmpDir)
	if err != nil {
		t.Fatalf("DiscoverExports failed: %v", err)
	}
	if len(refs) != 2 {
		t.Fatalf("expected 2 refs from directory walk, got %d", len(refs))
	}
}

func TestChatGPTDiscoverExports_EmptyPathDefaults(t *testing.T) {
	home := t.TempDir()
	downloads := filepath.Join(home, "Downloads")
	safeDir := filepath.Join(downloads, "safe")
	if err := os.MkdirAll(safeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	expected := filepath.Join(safeDir, "conversations.json")
	if err := os.WriteFile(expected, []byte("[]"), 0o644); err != nil {
		t.Fatal(err)
	}
	// A FIFO with the export name used to be enough to make discovery hang.
	fifo := filepath.Join(downloads, "conversations.json")
	if chatGPTFIFOAvailable {
		if err := makeChatGPTFIFO(fifo); err != nil {
			t.Fatal(err)
		}
	}

	// os.UserHomeDir reads HOME on Unix; isolate the default path from the
	// developer's real Downloads directory.
	t.Setenv("HOME", home)
	imp := NewChatGPTImporter()
	type result struct {
		refs []ExportRef
		err  error
	}
	finished := make(chan result, 1)
	go func() {
		refs, err := imp.DiscoverExports("")
		finished <- result{refs: refs, err: err}
	}()

	select {
	case got := <-finished:
		if got.err != nil {
			t.Fatalf("DiscoverExports failed: %v", got.err)
		}
		if len(got.refs) != 1 {
			t.Fatalf("expected 1 regular export, got %d", len(got.refs))
		}
		if got.refs[0].Path != expected {
			t.Errorf("Path = %q, want %q", got.refs[0].Path, expected)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("DiscoverExports did not complete within 2 seconds")
	}
}

func TestChatGPTDiscoverExports_SkipsSymlinkEscapesAndSpecialFiles(t *testing.T) {
	root := t.TempDir()
	outside := t.TempDir()
	outsideExport := filepath.Join(outside, "conversations.json")
	if err := os.WriteFile(outsideExport, []byte("[]"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(root, "escaped")); err != nil {
		t.Skipf("symlinks unavailable: %v", err)
	}

	safeDir := filepath.Join(root, "safe")
	if err := os.MkdirAll(safeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	expected := filepath.Join(safeDir, "conversations.json")
	if err := os.WriteFile(expected, []byte("[]"), 0o644); err != nil {
		t.Fatal(err)
	}
	fifo := filepath.Join(root, "conversations.json")
	if chatGPTFIFOAvailable {
		if err := makeChatGPTFIFO(fifo); err != nil {
			t.Fatal(err)
		}
	}

	refs, err := NewChatGPTImporter().DiscoverExports(root)
	if err != nil {
		t.Fatalf("DiscoverExports failed: %v", err)
	}
	if len(refs) != 1 {
		t.Fatalf("expected only the regular in-tree export, got %d refs", len(refs))
	}
	if refs[0].Path != expected {
		t.Errorf("Path = %q, want %q", refs[0].Path, expected)
	}
}

func TestChatGPTDiscoverExports_MissingPath(t *testing.T) {
	imp := NewChatGPTImporter()
	_, err := imp.DiscoverExports("/nonexistent/path")
	if err == nil {
		t.Error("expected error for nonexistent path, got nil")
	}
}

func TestChatGPTImportExport_Valid(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "conversations.json")
	data := `[{
		"title": "Test Chat",
		"create_time": 1700000000,
		"mapping": {
			"msg1": {
				"message": {
					"author": {"role": "user"},
					"content": {"parts": ["hello"]}
				}
			},
			"msg2": {
				"message": {
					"author": {"role": "assistant"},
					"content": {"parts": ["Hello! How can I help you today? This is a longer response that exceeds the fifty character minimum threshold for inclusion in the import."]}
				}
			}
		}
	}]`
	if err := os.WriteFile(path, []byte(data), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewChatGPTImporter()
	ref := ExportRef{
		Tool:       "chatgpt",
		Path:       path,
		Format:     "json",
		ModifiedAt: time.Now(),
	}
	facts, err := imp.ImportExport(ref)
	if err != nil {
		t.Fatalf("ImportExport failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact (only assistant with content>50), got %d", len(facts))
	}
	if facts[0].Source != "chatgpt" {
		t.Errorf("Source = %q, want %q", facts[0].Source, "chatgpt")
	}
	if facts[0].Content != "Hello! How can I help you today? This is a longer response that exceeds the fifty character minimum threshold for inclusion in the import." {
		t.Errorf("Content = %q, want %q", facts[0].Content, "Hello! How can I help you today? This is a longer response that exceeds the fifty character minimum threshold for inclusion in the import.")
	}
	if facts[0].Metadata["title"] != "Test Chat" {
		t.Errorf("title = %q, want %q", facts[0].Metadata["title"], "Test Chat")
	}
}

func TestChatGPTImportExport_ShortContent(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "conversations.json")
	// Assistant content is only 5 chars (< 50) — should be skipped.
	data := `[{
		"title": "Short",
		"create_time": 1700000000,
		"mapping": {
			"msg1": {
				"message": {
					"author": {"role": "assistant"},
					"content": {"parts": ["Short"]}
				}
			}
		}
	}]`
	if err := os.WriteFile(path, []byte(data), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewChatGPTImporter()
	ref := ExportRef{Tool: "chatgpt", Path: path, Format: "json"}
	facts, err := imp.ImportExport(ref)
	if err != nil {
		t.Fatalf("ImportExport failed: %v", err)
	}
	if len(facts) != 0 {
		t.Errorf("expected 0 facts for short content, got %d", len(facts))
	}
}

func TestChatGPTImportExport_NilMessage(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "conversations.json")
	data := `[{
		"title": "Nil test",
		"create_time": 1700000000,
		"mapping": {
			"msg1": {
				"message": null
			}
		}
	}]`
	if err := os.WriteFile(path, []byte(data), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewChatGPTImporter()
	ref := ExportRef{Tool: "chatgpt", Path: path, Format: "json"}
	facts, err := imp.ImportExport(ref)
	if err != nil {
		t.Fatalf("ImportExport failed: %v", err)
	}
	// nil message should be skipped without panic.
	if len(facts) != 0 {
		t.Errorf("expected 0 facts, got %d", len(facts))
	}
}

func TestChatGPTImportExport_InvalidJSON(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "conversations.json")
	if err := os.WriteFile(path, []byte("{invalid"), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewChatGPTImporter()
	ref := ExportRef{Tool: "chatgpt", Path: path, Format: "json"}
	_, err := imp.ImportExport(ref)
	if err == nil {
		t.Error("expected error for invalid JSON, got nil")
	}
	if err != nil && !strings.Contains(err.Error(), "failed to parse JSON") {
		t.Errorf("unexpected error: %v", err)
	}
}

func TestChatGPTImportExport_MissingFile(t *testing.T) {
	imp := NewChatGPTImporter()
	ref := ExportRef{Tool: "chatgpt", Path: "/nonexistent/file.json", Format: "json"}
	_, err := imp.ImportExport(ref)
	if err == nil {
		t.Error("expected error for missing file, got nil")
	}
}

func TestChatGPTImportExport_UserRoleIgnored(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "conversations.json")
	data := `[{
		"title": "User only",
		"create_time": 1700000000,
		"mapping": {
			"msg1": {
				"message": {
					"author": {"role": "user"},
					"content": {"parts": ["User message that is long enough to be included but should be ignored because role is user"]}
				}
			}
		}
	}]`
	if err := os.WriteFile(path, []byte(data), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewChatGPTImporter()
	ref := ExportRef{Tool: "chatgpt", Path: path, Format: "json"}
	facts, err := imp.ImportExport(ref)
	if err != nil {
		t.Fatalf("ImportExport failed: %v", err)
	}
	if len(facts) != 0 {
		t.Errorf("expected 0 facts (user role ignored), got %d", len(facts))
	}
}
