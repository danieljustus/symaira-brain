package vcs

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// writeFile is a tiny test helper that writes a file, failing the test.
func writeFile(t *testing.T, path, content string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
}

// logCount returns the number of commits on the current branch.
func logCount(t *testing.T, dir string) int {
	t.Helper()
	out, err := runGit(dir, "log", "--oneline")
	if err != nil {
		t.Fatalf("git log: %v", err)
	}
	trimmed := strings.TrimSpace(out)
	if trimmed == "" {
		return 0
	}
	return len(strings.Split(trimmed, "\n"))
}

// makeSkillDir creates a small skill-like tree and returns its path.
func makeSkillDir(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	writeFile(t, filepath.Join(dir, "SKILL.md"), "---\nname: vcs-test\ndescription: vcs test\n---\n\nBody v1.\n")
	writeFile(t, filepath.Join(dir, "references", "details.md"), "Details v1.\n")
	return dir
}

func TestInitCreatesRepoWithSingleInitialCommit(t *testing.T) {
	dir := makeSkillDir(t)

	created, err := Init(dir)
	if err != nil {
		t.Fatalf("Init: %v", err)
	}
	if !created {
		t.Fatal("expected Init to report created=true")
	}
	if !IsRepo(dir) {
		t.Fatal("expected dir to be a repository after Init")
	}
	if n := logCount(t, dir); n != 1 {
		t.Fatalf("expected exactly one commit, got %d", n)
	}
	out, err := runGit(dir, "log", "--format=%s")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(out, "import") {
		t.Fatalf("expected initial commit subject to mention import, got %q", out)
	}
}

func TestInitIsIdempotent(t *testing.T) {
	dir := makeSkillDir(t)
	if _, err := Init(dir); err != nil {
		t.Fatal(err)
	}
	created, err := Init(dir)
	if err != nil {
		t.Fatalf("second Init: %v", err)
	}
	if created {
		t.Fatal("expected second Init to report created=false")
	}
	if n := logCount(t, dir); n != 1 {
		t.Fatalf("expected still one commit, got %d", n)
	}
}

func TestCommitRecordsSecondCommitAndKeepsPreviousState(t *testing.T) {
	dir := makeSkillDir(t)
	if _, err := Init(dir); err != nil {
		t.Fatal(err)
	}
	first, err := Head(dir)
	if err != nil {
		t.Fatal(err)
	}

	writeFile(t, filepath.Join(dir, "SKILL.md"), "---\nname: vcs-test\ndescription: vcs test\n---\n\nBody v2.\n")
	hash, err := Commit(dir, "update: skill vcs-test from /tmp/src")
	if err != nil {
		t.Fatalf("Commit: %v", err)
	}
	if hash == "" {
		t.Fatal("expected a commit hash")
	}
	if n := logCount(t, dir); n != 2 {
		t.Fatalf("expected two commits, got %d", n)
	}
	// The first commit must still contain the original state.
	out, err := runGit(dir, "show", first+":SKILL.md")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(out, "Body v1.") {
		t.Fatalf("previous state lost: first commit contains %q", out)
	}
}

func TestCommitIsNoopWhenClean(t *testing.T) {
	dir := makeSkillDir(t)
	if _, err := Init(dir); err != nil {
		t.Fatal(err)
	}
	hash, err := Commit(dir, "update: nothing changed")
	if err != nil {
		t.Fatalf("Commit: %v", err)
	}
	if hash != "" {
		t.Fatalf("expected no-op commit to return empty hash, got %q", hash)
	}
	if n := logCount(t, dir); n != 1 {
		t.Fatalf("expected one commit, got %d", n)
	}
}

// TestCommitPreservesUserCommit locks the #118 invariant: a hand-made
// user commit inside a skill repo survives the next symskills write — no
// force, no reset, no history rewrite.
func TestCommitPreservesUserCommit(t *testing.T) {
	dir := makeSkillDir(t)
	if _, err := Init(dir); err != nil {
		t.Fatal(err)
	}
	// Simulate a user committing their own change with their own identity.
	writeFile(t, filepath.Join(dir, "SKILL.md"), "---\nname: vcs-test\ndescription: vcs test\n---\n\nUser edit.\n")
	if _, err := runGit(dir, append(commitFlags(), "commit", "-am", "user: hand-made edit")...); err != nil {
		t.Fatalf("user commit: %v", err)
	}
	userHead, err := Head(dir)
	if err != nil {
		t.Fatal(err)
	}

	// A symskills write now modifies the tree and commits on top.
	writeFile(t, filepath.Join(dir, "references", "details.md"), "Details edited by symskills.\n")
	if _, err := Commit(dir, "update: symskills write after user edit"); err != nil {
		t.Fatalf("symskills Commit: %v", err)
	}
	out, err := runGit(dir, "log", "--format=%H")
	if err != nil {
		t.Fatal(err)
	}
	lines := strings.Split(strings.TrimSpace(out), "\n")
	if len(lines) != 3 {
		t.Fatalf("expected 3 commits (init, user, symskills), got %d: %v", len(lines), lines)
	}
	if lines[1] != userHead {
		t.Fatalf("user commit was rewritten: want %s in history, got %v", userHead, lines)
	}
}

func TestDirty(t *testing.T) {
	dir := makeSkillDir(t)
	if _, err := Init(dir); err != nil {
		t.Fatal(err)
	}
	dirty, err := Dirty(dir)
	if err != nil {
		t.Fatal(err)
	}
	if dirty {
		t.Fatal("expected clean tree after initial commit")
	}
	writeFile(t, filepath.Join(dir, "SKILL.md"), "---\nname: vcs-test\ndescription: vcs test\n---\n\nDirty.\n")
	dirty, err = Dirty(dir)
	if err != nil {
		t.Fatal(err)
	}
	if !dirty {
		t.Fatal("expected dirty tree after edit")
	}
}

// TestUnavailableWhenGitMissing proves the degradation contract: with git
// off PATH every operation fails with ErrUnavailable and never with a
// partial side effect.
func TestUnavailableWhenGitMissing(t *testing.T) {
	t.Setenv("PATH", t.TempDir())
	if Available() {
		t.Fatal("expected Available()=false with git off PATH")
	}
	dir := makeSkillDir(t)
	if _, err := Init(dir); err == nil {
		t.Fatal("expected Init to fail with git missing")
	}
	if _, err := Commit(dir, "update: x"); err == nil {
		t.Fatal("expected Commit to fail with git missing")
	}
	if IsRepo(dir) {
		t.Fatal("expected IsRepo=false with git missing")
	}
}

func TestHead(t *testing.T) {
	dir := makeSkillDir(t)
	if _, err := Init(dir); err != nil {
		t.Fatal(err)
	}
	head, err := Head(dir)
	if err != nil {
		t.Fatal(err)
	}
	if len(head) != 40 {
		t.Fatalf("expected 40-char sha, got %q", head)
	}
}
