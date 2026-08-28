package git

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/importer"
)

// Test helpers

func initGitRepo(t *testing.T, dir string) {
	t.Helper()
	cmds := [][]string{
		{"init", "-b", "main"},
		{"config", "user.email", "test@example.com"},
		{"config", "user.name", "Test User"},
	}
	for _, args := range cmds {
		cmd := exec.Command("git", append([]string{"-C", dir}, args...)...)
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v failed: %v\n%s", args, err, out)
		}
	}
}

func writeAndCommit(t *testing.T, dir, filename, content, msg string) {
	t.Helper()
	fpath := filepath.Join(dir, filename)
	if err := os.WriteFile(fpath, []byte(content), 0o644); err != nil {
		t.Fatalf("writeFile %s: %v", filename, err)
	}
	cmd := exec.Command("git", "-C", dir, "add", ".")
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("git add: %v\n%s", err, out)
	}
	cmd = exec.Command("git", "-C", dir, "commit", "-m", msg)
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("git commit: %v\n%s", err, out)
	}
}

func TestNewGitImporter(t *testing.T) {
	imp := NewGitImporter("/tmp/test", "testuser")

	if imp.Name() != "git" {
		t.Errorf("Name() = %q, want %q", imp.Name(), "git")
	}

	if imp.Category() != "code" {
		t.Errorf("Category() = %q, want %q", imp.Category(), "code")
	}

	if imp.PrivacyLevel() != "public" {
		t.Errorf("PrivacyLevel() = %q, want %q", imp.PrivacyLevel(), "public")
	}

	if imp.RequiresPIIGuard() {
		t.Error("RequiresPIIGuard() = true, want false")
	}
}

func TestNewGitImporterDefaults(t *testing.T) {
	imp := NewGitImporter("", "")

	if imp.repoPath != "" {
		t.Errorf("repoPath = %q, want empty", imp.repoPath)
	}

	if imp.author != "" {
		t.Errorf("author = %q, want empty", imp.author)
	}
}

func TestDiscoverSessionsFindsCommits(t *testing.T) {
	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)
	writeAndCommit(t, tmpDir, "file1.txt", "hello", "Initial commit")
	writeAndCommit(t, tmpDir, "file2.txt", "world", "Second commit")

	imp := NewGitImporter(tmpDir, "")
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 2 {
		t.Fatalf("expected 2 sessions, got %d", len(sessions))
	}

	for _, s := range sessions {
		if s.Tool != "git" {
			t.Errorf("Tool = %q, want %q", s.Tool, "git")
		}
		if s.Path != tmpDir {
			t.Errorf("Path = %q, want %q", s.Path, tmpDir)
		}
		if s.Metadata["author"] != "Test User" {
			t.Errorf("author = %q, want %q", s.Metadata["author"], "Test User")
		}
	}
}

func TestDiscoverSessionsAuthorFilter(t *testing.T) {
	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	writeAndCommit(t, tmpDir, "file1.txt", "content1", "Commit by test user")

	// Commit with different author
	cmd := exec.Command("git", "-C", tmpDir, "config", "user.name", "Other Author")
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("git config: %v\n%s", err, out)
	}
	writeAndCommit(t, tmpDir, "file2.txt", "content2", "Commit by other author")

	imp := NewGitImporter(tmpDir, "Test User")
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 1 {
		t.Fatalf("expected 1 session (filtered by author), got %d", len(sessions))
	}
	if sessions[0].Metadata["author"] != "Test User" {
		t.Errorf("author = %q, want %q", sessions[0].Metadata["author"], "Test User")
	}
}

func TestDiscoverSessionsRespectsSince(t *testing.T) {
	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)
	writeAndCommit(t, tmpDir, "file1.txt", "old content", "Old commit")

	time.Sleep(2 * time.Second)
	// Git stores commit timestamps with second precision; leave a margin so
	// the new commit is unambiguously after the cutoff.
	since := time.Now().Add(-time.Second)

	writeAndCommit(t, tmpDir, "file2.txt", "new content", "New commit")

	imp := NewGitImporter(tmpDir, "")
	sessions, err := imp.DiscoverSessions(since)
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}

	if len(sessions) != 1 {
		t.Fatalf("expected 1 session (after since), got %d", len(sessions))
	}
	if sessions[0].Metadata["subject"] != "New commit" {
		t.Errorf("subject = %q, want %q", sessions[0].Metadata["subject"], "New commit")
	}
}

func TestDiscoverSessionsEmptyRepoReturnsError(t *testing.T) {
	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	// Empty repo (no commits) causes git log to fail with exit code 128.
	imp := NewGitImporter(tmpDir, "")
	_, err := imp.DiscoverSessions(time.Time{})
	if err == nil {
		t.Fatal("expected error for empty repo (no commits), got nil")
	}
	if !strings.Contains(err.Error(), "git log failed") {
		t.Errorf("expected git log failed error, got: %v", err)
	}
}

func TestDiscoverSessionsNonGitDir(t *testing.T) {
	tmpDir := t.TempDir()

	imp := NewGitImporter(tmpDir, "")
	_, err := imp.DiscoverSessions(time.Time{})
	if err == nil {
		t.Fatal("expected error for non-git directory, got nil")
	}
}

func TestDiscoverSessionsSubjectWithSpecialChars(t *testing.T) {
	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)
	writeAndCommit(t, tmpDir, "file1.txt", "content", "Subject with special chars: $HOME and backticks")

	imp := NewGitImporter(tmpDir, "")
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}
	if len(sessions) != 1 {
		t.Fatalf("expected 1 session, got %d", len(sessions))
	}
	if sessions[0].Metadata["subject"] != "Subject with special chars: $HOME and backticks" {
		t.Errorf("subject = %q, want %q", sessions[0].Metadata["subject"],
			"Subject with special chars: $HOME and backticks")
	}
}

func TestImportSessionParsesCommit(t *testing.T) {
	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	// First commit (root) — diff against ^ fails.
	writeAndCommit(t, tmpDir, "base.txt", "base", "Base commit")
	// Second commit — has a parent, so diff works.
	writeAndCommit(t, tmpDir, "main.go", `package main

func main() {
	println("hello")
}`, "Add main function")

	imp := NewGitImporter(tmpDir, "")
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}
	if len(sessions) != 2 {
		t.Fatalf("expected 2 sessions, got %d", len(sessions))
	}

	// Import the later session (the one with a parent commit).
	facts, err := imp.ImportSession(sessions[0])
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	if len(facts) != 1 {
		t.Fatalf("expected 1 fact, got %d", len(facts))
	}

	f := facts[0]
	if f.Source != "git" {
		t.Errorf("Source = %q, want %q", f.Source, "git")
	}
	if f.SessionID == "" {
		t.Error("SessionID should not be empty")
	}
	if f.Metadata["author"] != "Test User" {
		t.Errorf("author = %q, want %q", f.Metadata["author"], "Test User")
	}
	if f.Metadata["email"] != "test@example.com" {
		t.Errorf("email = %q, want %q", f.Metadata["email"], "test@example.com")
	}
	if f.Metadata["commit"] == "" {
		t.Error("commit hash should not be empty")
	}
	// Should have file changed info (since commit has a parent)
	if _, ok := f.Metadata["files_changed"]; !ok {
		t.Error("files_changed should be present in metadata")
	}
}

func TestImportSessionUnknownCommit(t *testing.T) {
	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	imp := NewGitImporter(tmpDir, "")
	ref := importer.SessionRef{
		Tool:      "git",
		SessionID: "0000000000000000000000000000000000000000",
		Path:      tmpDir,
	}

	_, err := imp.ImportSession(ref)
	if err == nil {
		t.Fatal("expected error for unknown commit, got nil")
	}
}

func TestImportSessionParseDiffStats(t *testing.T) {
	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	// First commit (root) — diff against ^ fails, no insertions/deletions.
	writeAndCommit(t, tmpDir, "base.txt", "base", "Base commit")
	// Second commit — diff works, should show insertions.
	writeAndCommit(t, tmpDir, "big.go", `package main

func main() {
	println("hello world")
	println("goodbye")
}
`, "Add big.go")

	imp := NewGitImporter(tmpDir, "")
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}
	if len(sessions) != 2 {
		t.Fatalf("expected 2 sessions, got %d", len(sessions))
	}

	// Import the second session (has a parent, diff works).
	facts, err := imp.ImportSession(sessions[0])
	if err != nil {
		t.Fatalf("ImportSession failed: %v", err)
	}

	if f := facts[0]; f.Metadata["insertions"] == "0" {
		t.Errorf("expected insertions > 0, got 0")
	}
}

func TestImportSessionMergeCommit(t *testing.T) {
	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)

	writeAndCommit(t, tmpDir, "main.go", "initial", "Initial commit")

	// Create a branch and merge it to produce a merge commit
	cmd := exec.Command("git", "-C", tmpDir, "checkout", "-b", "feature")
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("git checkout -b: %v\n%s", err, out)
	}
	writeAndCommit(t, tmpDir, "feature.go", "feature content", "Feature commit")

	cmd = exec.Command("git", "-C", tmpDir, "checkout", "main")
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("git checkout main: %v\n%s", err, out)
	}
	// Use --no-ff to force a merge commit
	cmd = exec.Command("git", "-C", tmpDir, "merge", "--no-ff", "-m", "Merge feature branch", "feature")
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("git merge: %v\n%s", err, out)
	}

	imp := NewGitImporter(tmpDir, "")
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}
	// --no-merges excludes the merge commit, so only initial + feature commits
	if len(sessions) != 2 {
		t.Fatalf("expected 2 sessions (no-merges excludes merge), got %d", len(sessions))
	}
}

func TestImportSessionWithAuthorFilter(t *testing.T) {
	tmpDir := t.TempDir()
	initGitRepo(t, tmpDir)
	writeAndCommit(t, tmpDir, "file1.txt", "content", "Commit 1")

	// Change author and commit again
	cmd := exec.Command("git", "-C", tmpDir, "config", "user.name", "Other Author")
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("git config: %v\n%s", err, out)
	}
	cmd = exec.Command("git", "-C", tmpDir, "config", "user.email", "other@example.com")
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("git config: %v\n%s", err, out)
	}
	writeAndCommit(t, tmpDir, "file2.txt", "more", "Commit 2 by other")

	// Filter by "Test User" should only find the first commit
	imp := NewGitImporter(tmpDir, "Test User")
	sessions, err := imp.DiscoverSessions(time.Time{})
	if err != nil {
		t.Fatalf("DiscoverSessions failed: %v", err)
	}
	if len(sessions) != 1 {
		t.Fatalf("expected 1 session filtered by author, got %d", len(sessions))
	}
	if sessions[0].Metadata["author"] != "Test User" {
		t.Errorf("author = %q, want %q", sessions[0].Metadata["author"], "Test User")
	}
}
