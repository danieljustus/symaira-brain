package codex

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

func TestNewCodexImporter(t *testing.T) {
	imp := NewCodexImporter("/tmp/codex")

	if imp.Name() != "codex" {
		t.Errorf("Name() = %q, want %q", imp.Name(), "codex")
	}
}

func TestNewCodexImporterDefaults(t *testing.T) {
	imp := NewCodexImporter("")

	if imp.customPath != "" {
		t.Errorf("customPath = %q, want empty", imp.customPath)
	}
}

func TestDiscoverSessionsFindsRolloutJSONL(t *testing.T) {
	tmpDir := t.TempDir()
	archivedDir := filepath.Join(tmpDir, "archived_sessions")
	if err := os.MkdirAll(archivedDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Write a rollout JSONL file.
	rolloutPath := filepath.Join(archivedDir, "rollout-abc123.jsonl")
	content := []byte(`{"timestamp":"2026-06-20T10:00:00Z","type":"message","payload":{"role":"user","content":"Hello"}}
{"timestamp":"2026-06-20T10:00:05Z","type":"message","payload":{"role":"assistant","content":"I can help you with that. Let me write a Go function to solve your problem."}}
`)
	if err := os.WriteFile(rolloutPath, content, 0o644); err != nil {
		t.Fatal(err)
	}

	// Write a session_index.jsonl that should be IGNORED.
	indexPath := filepath.Join(archivedDir, "session_index.jsonl")
	if err := os.WriteFile(indexPath, []byte(`{"session_id":"rollout-abc123"}`), 0o644); err != nil {
		t.Fatal(err)
	}

	// Write a non-rollout .jsonl file that should be IGNORED.
	otherPath := filepath.Join(archivedDir, "other-session.jsonl")
	if err := os.WriteFile(otherPath, []byte(`{"type":"message"}`), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewCodexImporter(tmpDir)
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 1 {
		t.Fatalf("expected 1 session, got %d", len(sessions))
	}

	s := sessions[0]
	if s.Tool != "codex" {
		t.Errorf("Tool = %q, want %q", s.Tool, "codex")
	}
	if s.SessionID != "rollout-abc123" {
		t.Errorf("SessionID = %q, want %q", s.SessionID, "rollout-abc123")
	}
}

func TestDiscoverSessionsRespectsSince(t *testing.T) {
	tmpDir := t.TempDir()
	archivedDir := filepath.Join(tmpDir, "archived_sessions")
	if err := os.MkdirAll(archivedDir, 0o755); err != nil {
		t.Fatal(err)
	}

	rolloutPath := filepath.Join(archivedDir, "rollout-old.jsonl")
	if err := os.WriteFile(rolloutPath, []byte(`{"type":"message","payload":{"role":"user","content":"hi"}}`), 0o644); err != nil {
		t.Fatal(err)
	}

	future := time.Now().AddDate(0, 0, 1) // tomorrow
	imp := NewCodexImporter(tmpDir)
	sessions, err := imp.DiscoverSessions(future)
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 0 {
		t.Fatalf("expected 0 sessions (all older than since), got %d", len(sessions))
	}
}

func TestDiscoverSessionsMissingDir(t *testing.T) {
	tmpDir := t.TempDir() // no archived_sessions dir

	imp := NewCodexImporter(tmpDir)
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions should not error on missing dir: %v", err)
	}

	if len(sessions) != 0 {
		t.Fatalf("expected 0 sessions when dir missing, got %d", len(sessions))
	}
}

func TestImportSessionParsesRolloutJSONL(t *testing.T) {
	tmpDir := t.TempDir()
	rolloutPath := filepath.Join(tmpDir, "rollout-test.jsonl")

	lines := `{"timestamp":"2026-06-20T10:00:00Z","type":"message","payload":{"role":"user","content":"Write a sorting algorithm"}}
{"timestamp":"2026-06-20T10:00:05Z","type":"message","payload":{"role":"assistant","content":"Here is a quicksort implementation in Go that sorts a slice of integers in ascending order. The algorithm picks a pivot element and partitions the array around it."}}
{"timestamp":"2026-06-20T10:00:10Z","type":"message","payload":{"role":"user","content":"Thanks"}}
`
	if err := os.WriteFile(rolloutPath, []byte(lines), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewCodexImporter("")
	ref := importer.SessionRef{
		Tool:       "codex",
		SessionID:  "rollout-test",
		Path:       rolloutPath,
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
	if f.Source != "codex" {
		t.Errorf("Source = %q, want %q", f.Source, "codex")
	}
	if f.SessionID != "rollout-test" {
		t.Errorf("SessionID = %q, want %q", f.SessionID, "rollout-test")
	}
	expectedTime, _ := time.Parse(time.RFC3339, "2026-06-20T10:00:05Z")
	if !f.Timestamp.Equal(expectedTime) {
		t.Errorf("Timestamp = %v, want %v", f.Timestamp, expectedTime)
	}
}

func TestImportSessionSkipsShortAndUserMessages(t *testing.T) {
	tmpDir := t.TempDir()
	rolloutPath := filepath.Join(tmpDir, "rollout-skip.jsonl")

	lines := `{"timestamp":"2026-06-20T10:00:00Z","type":"message","payload":{"role":"assistant","content":"Short"}}
{"timestamp":"2026-06-20T10:00:01Z","type":"message","payload":{"role":"user","content":"This is a long user message that should be skipped because we only import assistant messages for fact extraction."}}
{"timestamp":"2026-06-20T10:00:02Z","type":"message","payload":{"role":"assistant","content":"This is a sufficiently long assistant message that should be captured as an imported fact for the memory database."}}
`
	if err := os.WriteFile(rolloutPath, []byte(lines), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewCodexImporter("")
	ref := importer.SessionRef{
		Tool:       "codex",
		SessionID:  "rollout-skip",
		Path:       rolloutPath,
		ModifiedAt: time.Now(),
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

func TestImportSessionEmptyFile(t *testing.T) {
	tmpDir := t.TempDir()
	rolloutPath := filepath.Join(tmpDir, "rollout-empty.jsonl")
	if err := os.WriteFile(rolloutPath, []byte(""), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewCodexImporter("")
	ref := importer.SessionRef{
		Tool:       "codex",
		SessionID:  "rollout-empty",
		Path:       rolloutPath,
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
