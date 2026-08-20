package aider

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

func TestNewAiderImporter(t *testing.T) {
	imp := NewAiderImporter([]string{"/tmp/project1", "/tmp/project2"})

	if imp.Name() != "aider" {
		t.Errorf("Name() = %q, want %q", imp.Name(), "aider")
	}
}

func TestNewAiderImporterDefaults(t *testing.T) {
	imp := NewAiderImporter(nil)

	if len(imp.customPaths) != 0 {
		t.Errorf("customPaths = %v, want empty", imp.customPaths)
	}
}

func TestDiscoverSessionsFindsAiderHistoryFiles(t *testing.T) {
	tmpDir := t.TempDir()

	projectDir := filepath.Join(tmpDir, "my-project")
	if err := os.MkdirAll(projectDir, 0o755); err != nil {
		t.Fatal(err)
	}
	historyPath := filepath.Join(projectDir, ".aider.chat.history.md")
	if err := os.WriteFile(historyPath, []byte("some content"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create another file that should NOT be discovered.
	if err := os.WriteFile(filepath.Join(projectDir, "notes.md"), []byte("not a history file"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create a nested file that should also be found.
	nestedDir := filepath.Join(tmpDir, "other-project", "sub")
	if err := os.MkdirAll(nestedDir, 0o755); err != nil {
		t.Fatal(err)
	}
	nestedHistory := filepath.Join(nestedDir, ".aider.chat.history.md")
	if err := os.WriteFile(nestedHistory, []byte("more content"), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewAiderImporter([]string{tmpDir})
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 2 {
		t.Fatalf("expected 2 sessions, got %d", len(sessions))
	}

	for _, s := range sessions {
		if s.Tool != "aider" {
			t.Errorf("Tool = %q, want %q", s.Tool, "aider")
		}
		if _, err := os.Stat(s.Path); err != nil {
			t.Errorf("session path %q does not exist: %v", s.Path, err)
		}
	}
}

func TestDiscoverSessionsRespectsSince(t *testing.T) {
	tmpDir := t.TempDir()

	projectDir := filepath.Join(tmpDir, "old-project")
	if err := os.MkdirAll(projectDir, 0o755); err != nil {
		t.Fatal(err)
	}
	historyPath := filepath.Join(projectDir, ".aider.chat.history.md")
	if err := os.WriteFile(historyPath, []byte("content"), 0o644); err != nil {
		t.Fatal(err)
	}

	pastTime := time.Now().Add(-24 * time.Hour)
	if err := os.Chtimes(historyPath, pastTime, pastTime); err != nil {
		t.Fatal(err)
	}

	future := time.Now().Add(time.Hour)
	imp := NewAiderImporter([]string{tmpDir})
	sessions, err := imp.DiscoverSessions(future)
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 0 {
		t.Fatalf("expected 0 sessions (all older than since), got %d", len(sessions))
	}
}

func TestDiscoverSessionsWithExplicitPath(t *testing.T) {
	home, err := os.UserHomeDir()
	if err != nil {
		t.Fatalf("UserHomeDir() should work: %v", err)
	}
	if home == "" {
		t.Fatal("expected non-empty home directory")
	}

	tmpDir := t.TempDir()
	imp := NewAiderImporter([]string{tmpDir})
	sessions, err := imp.DiscoverSessions(time.Now().Add(24 * time.Hour))
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}
	if len(sessions) != 0 {
		t.Fatalf("expected 0 sessions, got %d", len(sessions))
	}
}

func TestDiscoverSessionsNonExistentPath(t *testing.T) {
	imp := NewAiderImporter([]string{"/tmp/nonexistent-dir-12345"})
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions should not error on non-existent path: %v", err)
	}
	if len(sessions) != 0 {
		t.Fatalf("expected 0 sessions from non-existent path, got %d", len(sessions))
	}
}

func TestDiscoverSessionsNoHistoryFiles(t *testing.T) {
	tmpDir := t.TempDir()

	projectDir := filepath.Join(tmpDir, "some-project")
	if err := os.MkdirAll(projectDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(projectDir, "README.md"), []byte("# Project"), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewAiderImporter([]string{tmpDir})
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 0 {
		t.Fatalf("expected 0 sessions when no history files exist, got %d", len(sessions))
	}
}

// ImportSession tests: The parser captures content on lines AFTER the
// **Assistant**: marker line, not on the same line. Content is accumulated
// until the next heading or role marker.

func TestImportSessionParsesAiderHistory(t *testing.T) {
	tmpDir := t.TempDir()
	historyPath := filepath.Join(tmpDir, ".aider.chat.history.md")

	content := `# aider chat started at 2026-07-20 10:00:00

## First message

**Human**: Can you write a sorting algorithm for me?
**Assistant**: 
Here is a quicksort implementation in Go that sorts a slice of integers in ascending order. The algorithm picks a pivot element and partitions the array around it recursively.

**Human**: Thanks, that looks great!
**Assistant**: 
You're welcome! If you need any more help with sorting algorithms or optimization, feel free to ask. I'm happy to assist with data structures and algorithms.
`
	if err := os.WriteFile(historyPath, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	now := time.Now()
	imp := NewAiderImporter(nil)
	ref := importer.SessionRef{
		Tool:       "aider",
		SessionID:  tmpDir,
		Path:       historyPath,
		ModifiedAt: now,
		Metadata: map[string]string{
			"project": tmpDir,
		},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	if len(facts) != 2 {
		t.Fatalf("expected 2 facts, got %d", len(facts))
	}

	for _, f := range facts {
		if f.Source != "aider" {
			t.Errorf("Source = %q, want %q", f.Source, "aider")
		}
		if f.SessionID != tmpDir {
			t.Errorf("SessionID = %q, want %q", f.SessionID, tmpDir)
		}
		if len(f.Content) <= 50 {
			t.Errorf("fact content should be >50 chars, got %d", len(f.Content))
		}
		if f.Metadata["project"] != tmpDir {
			t.Errorf("Metadata[project] = %q, want %q", f.Metadata["project"], tmpDir)
		}
	}
}

func TestImportSessionSkipsShortAssistantMessages(t *testing.T) {
	tmpDir := t.TempDir()
	historyPath := filepath.Join(tmpDir, ".aider.chat.history.md")

	content := "**Assistant**: \nOK\n**Assistant**: \nThis is a sufficiently long assistant message that should be captured as an imported fact for the aider memory importer.\n"
	if err := os.WriteFile(historyPath, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	now := time.Now()
	imp := NewAiderImporter(nil)
	ref := importer.SessionRef{
		Tool:       "aider",
		SessionID:  tmpDir,
		Path:       historyPath,
		ModifiedAt: now,
		Metadata:   map[string]string{},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	if len(facts) != 1 {
		t.Fatalf("expected 1 fact (short one skipped), got %d", len(facts))
	}
}

func TestImportSessionSkipsHumanMessages(t *testing.T) {
	tmpDir := t.TempDir()
	historyPath := filepath.Join(tmpDir, ".aider.chat.history.md")

	content := "**Human**: \nThis is a really long human message that asks a good question about how to implement various things in Go that could be very helpful.\n"
	if err := os.WriteFile(historyPath, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	now := time.Now()
	imp := NewAiderImporter(nil)
	ref := importer.SessionRef{
		Tool:       "aider",
		SessionID:  tmpDir,
		Path:       historyPath,
		ModifiedAt: now,
		Metadata:   map[string]string{},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	if len(facts) != 0 {
		t.Fatalf("expected 0 facts (human messages only), got %d", len(facts))
	}
}

func TestImportSessionEmptyFile(t *testing.T) {
	tmpDir := t.TempDir()
	historyPath := filepath.Join(tmpDir, ".aider.chat.history.md")

	if err := os.WriteFile(historyPath, []byte(""), 0o644); err != nil {
		t.Fatal(err)
	}

	now := time.Now()
	imp := NewAiderImporter(nil)
	ref := importer.SessionRef{
		Tool:       "aider",
		SessionID:  tmpDir,
		Path:       historyPath,
		ModifiedAt: now,
		Metadata:   map[string]string{},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	if len(facts) != 0 {
		t.Fatalf("expected 0 facts from empty file, got %d", len(facts))
	}
}

func TestImportSessionFileNotFound(t *testing.T) {
	imp := NewAiderImporter(nil)
	ref := importer.SessionRef{
		Tool:       "aider",
		SessionID:  "some-session",
		Path:       "/tmp/does-not-exist-aider-test-12345.md",
		ModifiedAt: time.Now(),
		Metadata:   map[string]string{},
	}

	_, err := imp.ImportSession(ref)
	if err == nil {
		t.Fatal("expected error for non-existent file, got nil")
	}
}

func TestImportSessionOnlyHeadings(t *testing.T) {
	tmpDir := t.TempDir()
	historyPath := filepath.Join(tmpDir, ".aider.chat.history.md")

	content := "# aider chat started at 2026-07-20 10:00:00\n\n## First message\n\n## Second message\n\n# New section\n"
	if err := os.WriteFile(historyPath, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	now := time.Now()
	imp := NewAiderImporter(nil)
	ref := importer.SessionRef{
		Tool:       "aider",
		SessionID:  tmpDir,
		Path:       historyPath,
		ModifiedAt: now,
		Metadata:   map[string]string{},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	if len(facts) != 0 {
		t.Fatalf("expected 0 facts from file with only headings, got %d", len(facts))
	}
}

func TestImportSessionMultipleAssistantBlocksWithHeadings(t *testing.T) {
	tmpDir := t.TempDir()
	historyPath := filepath.Join(tmpDir, ".aider.chat.history.md")

	content := `# Session 1

**Human**: Can you help me with Go?
**Assistant**: 
Sure! I can help you with Go programming. Here's an example of a concurrent worker pool implementation that uses goroutines and channels to process tasks efficiently.

## Checkpoint

**Human**: What about error handling?
**Assistant**: 
Error handling in Go is explicit and follows the pattern of returning errors as values. Here's how you can properly handle errors in a robust way with proper context wrapping and structured error types.

## Summary
`
	if err := os.WriteFile(historyPath, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	now := time.Now()
	imp := NewAiderImporter(nil)
	ref := importer.SessionRef{
		Tool:       "aider",
		SessionID:  tmpDir,
		Path:       historyPath,
		ModifiedAt: now,
		Metadata:   map[string]string{},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	if len(facts) != 2 {
		t.Fatalf("expected 2 facts, got %d", len(facts))
	}
}

func TestImportSessionTrailingAssistantContent(t *testing.T) {
	tmpDir := t.TempDir()
	historyPath := filepath.Join(tmpDir, ".aider.chat.history.md")

	// Content where the last block is an assistant message with content.
	// The post-loop check should capture it.
	content := "**Assistant**: \nThis is a sufficiently long assistant message at the end of the file that should be captured even without a following marker.\n"
	if err := os.WriteFile(historyPath, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	now := time.Now()
	imp := NewAiderImporter(nil)
	ref := importer.SessionRef{
		Tool:       "aider",
		SessionID:  tmpDir,
		Path:       historyPath,
		ModifiedAt: now,
		Metadata:   map[string]string{},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	if len(facts) != 1 {
		t.Fatalf("expected 1 fact (trailing assistant content), got %d", len(facts))
	}
}

func TestImportSessionContentBeforeMarker(t *testing.T) {
	tmpDir := t.TempDir()
	historyPath := filepath.Join(tmpDir, ".aider.chat.history.md")

	// Text before any **Human**: or **Assistant**: marker should not be captured
	// because currentRole is empty.
	content := "# aider chat\n\nSome introductory text without a role marker.\n\n**Assistant**: \nThis is a sufficiently long assistant message that should be captured as an imported fact for testing purposes in the aider importer.\n"
	if err := os.WriteFile(historyPath, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	now := time.Now()
	imp := NewAiderImporter(nil)
	ref := importer.SessionRef{
		Tool:       "aider",
		SessionID:  tmpDir,
		Path:       historyPath,
		ModifiedAt: now,
		Metadata:   map[string]string{},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}
}

func TestImportSessionMultiLineAssistantContent(t *testing.T) {
	tmpDir := t.TempDir()
	historyPath := filepath.Join(tmpDir, ".aider.chat.history.md")

	// Assistant message spanning multiple lines.
	content := "**Assistant**: \nHere is the first line of the assistant response.\n\nHere is the second line with more explanation.\n\nAnd a third line to ensure multi-line content is fully captured.\n"
	if err := os.WriteFile(historyPath, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	now := time.Now()
	imp := NewAiderImporter(nil)
	ref := importer.SessionRef{
		Tool:       "aider",
		SessionID:  tmpDir,
		Path:       historyPath,
		ModifiedAt: now,
		Metadata:   map[string]string{},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}

	if len(facts) > 0 {
		// Should contain content from all three lines.
		if !contains(facts[0].Content, "first line") {
			t.Errorf("fact should contain first line content")
		}
		if !contains(facts[0].Content, "second line") {
			t.Errorf("fact should contain second line content")
		}
		if !contains(facts[0].Content, "third line") {
			t.Errorf("fact should contain third line content")
		}
	}
}

func contains(s, substr string) bool {
	return len(s) >= len(substr) && containsStr(s, substr)
}

func containsStr(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}
