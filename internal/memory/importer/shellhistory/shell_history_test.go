package shellhistory

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

func TestNewShellHistoryImporter(t *testing.T) {
	imp := NewShellHistoryImporter("/tmp/history", true, []string{"git"})

	if imp.Name() != "shell-history" {
		t.Errorf("Name() = %q, want %q", imp.Name(), "shell-history")
	}

	if imp.Category() != "code" {
		t.Errorf("Category() = %q, want %q", imp.Category(), "code")
	}

	if imp.PrivacyLevel() != "confidential" {
		t.Errorf("PrivacyLevel() = %q, want %q", imp.PrivacyLevel(), "confidential")
	}

	if !imp.RequiresPIIGuard() {
		t.Error("RequiresPIIGuard() = false, want true")
	}
}

func TestNewShellHistoryImporterDefaults(t *testing.T) {
	imp := NewShellHistoryImporter("", false, nil)

	if imp.successOnly {
		t.Error("successOnly = true, want false")
	}

	if len(imp.filters) != 0 {
		t.Errorf("filters = %v, want empty", imp.filters)
	}

	if imp.minDurationMs != 1000 {
		t.Errorf("minDurationMs = %d, want 1000", imp.minDurationMs)
	}
}

func TestDiscoverSessionsEmptyPath(t *testing.T) {
	imp := NewShellHistoryImporter("", true, nil)
	imp.historyPath = "" // ensure empty

	_, err := imp.DiscoverSessions(time.Time{})
	if err == nil {
		t.Fatal("expected error for empty history path, got nil")
	}
}

func TestDiscoverSessionsMissingFile(t *testing.T) {
	tmpDir := t.TempDir()
	imp := NewShellHistoryImporter(filepath.Join(tmpDir, "nonexistent"), true, nil)

	_, err := imp.DiscoverSessions(time.Time{})
	if err == nil {
		t.Fatal("expected error for missing history file, got nil")
	}
}

func TestDiscoverSessionsParsesZshHistory(t *testing.T) {
	tmpDir := t.TempDir()
	histFile := filepath.Join(tmpDir, ".zsh_history")
	content := ": 1700000000:0;git status\n: 1700000001:0;npm install\n"
	if err := os.WriteFile(histFile, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewShellHistoryImporter(histFile, true, nil)
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 2 {
		t.Fatalf("expected 2 sessions, got %d", len(sessions))
	}

	if sessions[0].Tool != "shell-history" {
		t.Errorf("Tool = %q, want %q", sessions[0].Tool, "shell-history")
	}
	if sessions[0].Metadata["command"] != "git status" {
		t.Errorf("command = %q, want %q", sessions[0].Metadata["command"], "git status")
	}
}

func TestDiscoverSessionsRespectsSince(t *testing.T) {
	tmpDir := t.TempDir()
	histFile := filepath.Join(tmpDir, ".zsh_history")
	// Two entries: one old, one recent.
	content := ": 1000000000:0;old command\n: 2000000000:0;new command\n"
	if err := os.WriteFile(histFile, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewShellHistoryImporter(histFile, true, nil)

	// Only include entries after 1500000000
	since := time.Unix(1500000000, 0)
	sessions, err := imp.DiscoverSessions(since)
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 1 {
		t.Fatalf("expected 1 session (after since), got %d", len(sessions))
	}
	if sessions[0].Metadata["command"] != "new command" {
		t.Errorf("command = %q, want %q", sessions[0].Metadata["command"], "new command")
	}
}

func TestDiscoverSessionsExcludesCommonCommands(t *testing.T) {
	tmpDir := t.TempDir()
	histFile := filepath.Join(tmpDir, ".zsh_history")
	content := ": 1700000000:0;cd /tmp\n: 1700000001:0;git status\n: 1700000002:0;ls -la\n"
	if err := os.WriteFile(histFile, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewShellHistoryImporter(histFile, true, nil)
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	// "cd" and "ls" are excluded, only "git status" should remain.
	if len(sessions) != 1 {
		t.Fatalf("expected 1 session (cd and ls excluded), got %d", len(sessions))
	}
}

func TestDiscoverSessionsWithFilter(t *testing.T) {
	tmpDir := t.TempDir()
	histFile := filepath.Join(tmpDir, ".zsh_history")
	content := ": 1700000000:0;git status\n: 1700000001:0;npm install\n: 1700000002:0;docker ps\n"
	if err := os.WriteFile(histFile, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	imp := NewShellHistoryImporter(histFile, true, []string{"git"})
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 1 {
		t.Fatalf("expected 1 session (only git commands match filter), got %d", len(sessions))
	}
}

func TestParseHistoryLine_ZshExtended(t *testing.T) {
	ts, cmd := parseHistoryLine(": 1700000000:0;git status")
	if ts.Unix() != 1700000000 {
		t.Errorf("timestamp = %d, want 1700000000", ts.Unix())
	}
	if cmd != "git status" {
		t.Errorf("command = %q, want %q", cmd, "git status")
	}
}

func TestParseHistoryLine_BashFormat(t *testing.T) {
	ts, cmd := parseHistoryLine("#1700000000")
	if ts.Unix() != 1700000000 {
		t.Errorf("timestamp = %d, want 1700000000", ts.Unix())
	}
	if cmd != "" {
		t.Errorf("command = %q, want empty", cmd)
	}
}

func TestParseHistoryLine_InvalidLine(t *testing.T) {
	ts, cmd := parseHistoryLine("some random text")
	if !ts.IsZero() {
		t.Errorf("expected zero time, got %v", ts)
	}
	if cmd != "some random text" {
		t.Errorf("command = %q, want %q", cmd, "some random text")
	}
}

func TestParseHistoryLine_EmptyLine(t *testing.T) {
	ts, cmd := parseHistoryLine("")
	if !ts.IsZero() {
		t.Errorf("expected zero time, got %v", ts)
	}
	if cmd != "" {
		t.Errorf("command = %q, want empty", cmd)
	}
}

func TestImportSession(t *testing.T) {
	imp := NewShellHistoryImporter("/dev/null", true, nil)
	ref := importer.SessionRef{
		Tool:      "shell-history",
		SessionID: "1700000000",
		Metadata: map[string]string{
			"command": "git status",
		},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}
	if facts[0].Metadata["tag"] != "vcs" {
		t.Errorf("tag = %q, want %q", facts[0].Metadata["tag"], "vcs")
	}
}

func TestImportSessionEmptyCommand(t *testing.T) {
	imp := NewShellHistoryImporter("/dev/null", true, nil)
	ref := importer.SessionRef{
		Tool:      "shell-history",
		SessionID: "1700000000",
		Metadata: map[string]string{
			"command": "",
		},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if facts != nil {
		t.Errorf("expected nil facts for empty command, got %v", facts)
	}
}

func TestImportSessionNoTag(t *testing.T) {
	imp := NewShellHistoryImporter("/dev/null", true, nil)
	ref := importer.SessionRef{
		Tool:      "shell-history",
		SessionID: "1700000000",
		Metadata: map[string]string{
			"command": "some_unknown_tool --flag",
		},
	}

	facts, err := imp.ImportSession(ref)
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}
	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}
	if _, ok := facts[0].Metadata["tag"]; ok {
		t.Errorf("unexpected tag for unknown tool")
	}
}

func TestDetectHistoryPath_Zsh(t *testing.T) {
	t.Setenv("SHELL", "/bin/zsh")

	// Create a temp .zsh_history so detectHistoryPath finds it.
	home, err := os.UserHomeDir()
	if err != nil {
		t.Fatal(err)
	}
	origPath := filepath.Join(home, ".zsh_history")

	// If the real .zsh_history exists, detectHistoryPath should return it.
	path := detectHistoryPath()
	if path == "" || strings.HasSuffix(path, ".bash_history") {
		t.Skip("no .zsh_history found (expected on CI or clean home)")
	}
	if path != origPath {
		t.Errorf("detectHistoryPath() = %q, want %q", path, origPath)
	}
}

func TestDetectHistoryPath_Fallback(t *testing.T) {
	t.Setenv("SHELL", "/bin/sh") // not zsh

	home, err := os.UserHomeDir()
	if err != nil {
		t.Fatal(err)
	}
	expected := filepath.Join(home, ".bash_history")
	path := detectHistoryPath()
	if path != expected {
		t.Errorf("detectHistoryPath() = %q, want %q", path, expected)
	}
}

func TestMatchesFilter(t *testing.T) {
	imp := NewShellHistoryImporter("/dev/null", true, []string{"git", "docker"})

	if !imp.matchesFilter("git status") {
		t.Error("matchesFilter('git status') = false, want true")
	}
	if !imp.matchesFilter("docker ps") {
		t.Error("matchesFilter('docker ps') = false, want true")
	}
	if imp.matchesFilter("npm install") {
		t.Error("matchesFilter('npm install') = true, want false")
	}
}

func TestMatchesFilterNoFilter(t *testing.T) {
	imp := NewShellHistoryImporter("/dev/null", true, nil)

	if !imp.matchesFilter("anything") {
		t.Error("matchesFilter('anything') with no filters = false, want true")
	}
}

func TestTagCommand_EmptyCmd(t *testing.T) {
	if tag := tagCommand(""); tag != "" {
		t.Errorf("tagCommand('') = %q, want empty", tag)
	}
}

func TestTagCommand_Unknown(t *testing.T) {
	if tag := tagCommand("somecustomtool"); tag != "" {
		t.Errorf("tagCommand('somecustomtool') = %q, want empty", tag)
	}
}

func TestTagCommand_Known(t *testing.T) {
	tests := []struct {
		cmd  string
		want string
	}{
		{"git status", "vcs"},
		{"npm install", "package-manager"},
		{"brew install foo", "package-manager"},
		{"docker ps", "container"},
		{"make build", "build"},
		{"ls -la", ""},
		{"/usr/bin/git log", "vcs"}, // full path binary
	}

	for _, tt := range tests {
		t.Run(tt.cmd, func(t *testing.T) {
			got := tagCommand(tt.cmd)
			if got != tt.want {
				t.Errorf("tagCommand(%q) = %q, want %q", tt.cmd, got, tt.want)
			}
		})
	}
}

func TestDiscoverSessions_ScannerErrorHandling(t *testing.T) {
	// A directory as history file causes a read error during scanning.
	tmpDir := t.TempDir()
	imp := NewShellHistoryImporter(tmpDir, true, nil)

	_, err := imp.DiscoverSessions(time.Time{})
	if err == nil {
		// Opening a directory may or may not error depending on OS.
		// We only care that it doesn't panic.
		t.Log("opening a directory as history file did not error (acceptable)")
	}
}
