package claudecode

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

func TestNewClaudeCodeImporter(t *testing.T) {
	imp := NewClaudeCodeImporter("/tmp/claude")

	if imp.Name() != "claude-code" {
		t.Errorf("Name() = %q, want %q", imp.Name(), "claude-code")
	}
}

func TestNewClaudeCodeImporterDefaults(t *testing.T) {
	imp := NewClaudeCodeImporter("")

	if imp.customPath != "" {
		t.Errorf("customPath = %q, want empty", imp.customPath)
	}
}

func TestDiscoverSessionsFindsJSONLFiles(t *testing.T) {
	// Create temp directory structure mimicking ~/.claude/projects/
	tmpDir := t.TempDir()
	projectDir := filepath.Join(tmpDir, "project-abc")
	if err := os.MkdirAll(projectDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Write a .jsonl file.
	jsonlPath := filepath.Join(projectDir, "session-001.jsonl")
	content := []byte(`{"uuid":"u1","type":"human","message":{"role":"user","content":"Hello"},"timestamp":"2026-06-20T10:00:00Z"}
{"uuid":"u2","type":"assistant","message":{"role":"assistant","content":"Hi there, how can I help you with your code today?"},"timestamp":"2026-06-20T10:00:01Z"}
`)
	if err := os.WriteFile(jsonlPath, content, 0o644); err != nil {
		t.Fatal(err)
	}

	// Also write a .json file that should be IGNORED.
	jsonPath := filepath.Join(projectDir, "old-session.json")
	if err := os.WriteFile(jsonPath, []byte(`{"messages":[]}`), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewClaudeCodeImporter(tmpDir)
	sessions, err := imp.DiscoverSessions(time.Time{}) // since = zero = find all
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 1 {
		t.Fatalf("expected 1 session, got %d", len(sessions))
	}

	s := sessions[0]
	if s.Tool != "claude-code" {
		t.Errorf("Tool = %q, want %q", s.Tool, "claude-code")
	}
	if s.SessionID != "session-001" {
		t.Errorf("SessionID = %q, want %q", s.SessionID, "session-001")
	}
	if s.Path != jsonlPath {
		t.Errorf("Path = %q, want %q", s.Path, jsonlPath)
	}
}

func TestDiscoverSessionsRespectsSince(t *testing.T) {
	tmpDir := t.TempDir()
	projectDir := filepath.Join(tmpDir, "proj")
	if err := os.MkdirAll(projectDir, 0o755); err != nil {
		t.Fatal(err)
	}

	jsonlPath := filepath.Join(projectDir, "old.jsonl")
	if err := os.WriteFile(jsonlPath, []byte(`{"type":"human","message":{"role":"user","content":"hi"}}`), 0o644); err != nil {
		t.Fatal(err)
	}

	// Set modification time to 2020.
	future := time.Now().AddDate(0, 0, 1) // tomorrow
	imp := NewClaudeCodeImporter(tmpDir)
	sessions, err := imp.DiscoverSessions(future)
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 0 {
		t.Fatalf("expected 0 sessions (all older than since), got %d", len(sessions))
	}
}

func TestImportSessionParsesJSONL(t *testing.T) {
	tmpDir := t.TempDir()
	jsonlPath := filepath.Join(tmpDir, "test-transcript.jsonl")

	lines := `{"uuid":"u1","type":"human","message":{"role":"user","content":"Write a Go function"},"timestamp":"2026-06-20T10:00:00Z"}
{"uuid":"u2","type":"assistant","message":{"role":"assistant","content":"Here is a Go function that implements a binary search algorithm. It takes a sorted slice and a target value, returning the index if found or -1 if not found."},"timestamp":"2026-06-20T10:00:05Z","project":"my-project","model":"claude-sonnet-4-20250514"}
{"uuid":"u3","type":"human","message":{"role":"user","content":"Thanks"},"timestamp":"2026-06-20T10:00:10Z"}
`
	if err := os.WriteFile(jsonlPath, []byte(lines), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewClaudeCodeImporter("")
	ref := SessionRef{
		Tool:       "claude-code",
		SessionID:  "test-transcript",
		Path:       jsonlPath,
		ModifiedAt: time.Now(),
		Metadata:   map[string]string{},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	if len(facts) != 1 {
		t.Fatalf("expected 1 fact (assistant msg >50 chars), got %d", len(facts))
	}

	f := facts[0]
	if f.Source != "claude-code" {
		t.Errorf("Source = %q, want %q", f.Source, "claude-code")
	}
	if f.SessionID != "test-transcript" {
		t.Errorf("SessionID = %q, want %q", f.SessionID, "test-transcript")
	}
	if f.Metadata["project"] != "my-project" {
		t.Errorf("Metadata[project] = %q, want %q", f.Metadata["project"], "my-project")
	}
	if f.Metadata["model"] != "claude-sonnet-4-20250514" {
		t.Errorf("Metadata[model] = %q, want %q", f.Metadata["model"], "claude-sonnet-4-20250514")
	}
	expectedTime, _ := time.Parse(time.RFC3339, "2026-06-20T10:00:05Z")
	if !f.Timestamp.Equal(expectedTime) {
		t.Errorf("Timestamp = %v, want %v", f.Timestamp, expectedTime)
	}
}

func TestImportSessionSkipsShortMessages(t *testing.T) {
	tmpDir := t.TempDir()
	jsonlPath := filepath.Join(tmpDir, "short.jsonl")

	lines := `{"type":"assistant","message":{"role":"assistant","content":"Short"},"timestamp":"2026-06-20T10:00:00Z"}
{"type":"assistant","message":{"role":"assistant","content":"This is a long enough message that should be captured as a fact for testing purposes."},"timestamp":"2026-06-20T10:00:01Z"}
`
	if err := os.WriteFile(jsonlPath, []byte(lines), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewClaudeCodeImporter("")
	ref := importer.SessionRef{
		Tool:       "claude-code",
		SessionID:  "short",
		Path:       jsonlPath,
		ModifiedAt: time.Now(),
		Metadata:   map[string]string{},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	if len(facts) != 1 {
		t.Fatalf("expected 1 fact (only long msg), got %d", len(facts))
	}
}

func TestImportSessionEmptyFile(t *testing.T) {
	tmpDir := t.TempDir()
	jsonlPath := filepath.Join(tmpDir, "empty.jsonl")
	if err := os.WriteFile(jsonlPath, []byte(""), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewClaudeCodeImporter("")
	ref := importer.SessionRef{
		Tool:       "claude-code",
		SessionID:  "empty",
		Path:       jsonlPath,
		ModifiedAt: time.Now(),
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

// SessionRef alias for readability in tests.
type SessionRef = importer.SessionRef
