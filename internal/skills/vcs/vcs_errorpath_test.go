package vcs

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestRestoreRejectsNonRepo(t *testing.T) {
	dir := t.TempDir()
	if _, err := Restore(dir, t.TempDir(), "msg"); err == nil || !strings.Contains(err.Error(), "not a git repository") {
		t.Fatalf("expected not-a-repo error, got %v", err)
	}
}

func TestRestoreFailsWhenSourceMissing(t *testing.T) {
	dir := t.TempDir()
	if _, err := Init(dir); err != nil {
		t.Fatal(err)
	}
	if _, err := Commit(dir, "initial"); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "keep.txt"), []byte("keep\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := Commit(dir, "add keep"); err != nil {
		t.Fatal(err)
	}
	if _, err := Restore(dir, filepath.Join(t.TempDir(), "does-not-exist"), "restore"); err == nil {
		t.Fatal("expected copy error for missing source")
	}
	// The working tree must be untouched after the failed restore.
	if data, err := os.ReadFile(filepath.Join(dir, "keep.txt")); err != nil || string(data) != "keep\n" {
		t.Fatalf("working tree modified after failed restore: %q %v", data, err)
	}
}

func TestRestoreEmptySourceProducesNoCommit(t *testing.T) {
	dir := t.TempDir()
	if _, err := Init(dir); err != nil {
		t.Fatal(err)
	}
	if _, err := Commit(dir, "initial"); err != nil {
		t.Fatal(err)
	}
	empty := t.TempDir()
	hash, err := Restore(dir, empty, "restore to empty")
	if err != nil {
		t.Fatal(err)
	}
	if hash != "" {
		t.Fatalf("expected no commit for empty result tree, got %q", hash)
	}
}

func TestRestoreReplacesTreeAndCommits(t *testing.T) {
	dir := t.TempDir()
	if _, err := Init(dir); err != nil {
		t.Fatal(err)
	}
	if _, err := Commit(dir, "initial"); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "old.txt"), []byte("old\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := Commit(dir, "add old"); err != nil {
		t.Fatal(err)
	}
	src := t.TempDir()
	if err := os.WriteFile(filepath.Join(src, "new.txt"), []byte("new\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(src, "sub"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(src, "sub", "nested.md"), []byte("nested\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	hash, err := Restore(dir, src, "restore from snapshot")
	if err != nil {
		t.Fatal(err)
	}
	if len(hash) != 40 {
		t.Fatalf("expected full commit hash, got %q", hash)
	}
	if _, err := os.Stat(filepath.Join(dir, "old.txt")); !os.IsNotExist(err) {
		t.Fatalf("old.txt should be removed, got %v", err)
	}
	if data, err := os.ReadFile(filepath.Join(dir, "new.txt")); err != nil || string(data) != "new\n" {
		t.Fatalf("new.txt missing: %q %v", data, err)
	}
	if data, err := os.ReadFile(filepath.Join(dir, "sub", "nested.md")); err != nil || string(data) != "nested\n" {
		t.Fatalf("nested.md missing: %q %v", data, err)
	}
	// .git must be preserved.
	if !IsRepo(dir) {
		t.Fatal(".git not preserved by Restore")
	}
}

func TestEnsureParentInsideRefusesSymlinkEscape(t *testing.T) {
	root := t.TempDir()
	outside := t.TempDir()
	sub := filepath.Join(root, "sub")
	if err := os.Symlink(outside, sub); err != nil {
		t.Fatal(err)
	}
	err := ensureParentInside(root, filepath.Join(sub, "file.txt"))
	if err == nil || !strings.Contains(err.Error(), "escapes destination via symlink") {
		t.Fatalf("expected symlink-escape refusal, got %v", err)
	}
}

func TestEnsureParentInsideRefusesOutsidePath(t *testing.T) {
	root := t.TempDir()
	outside := t.TempDir()
	err := ensureParentInside(root, filepath.Join(outside, "file.txt"))
	if err == nil || !strings.Contains(err.Error(), "escapes destination") {
		t.Fatalf("expected outside-path refusal, got %v", err)
	}
}

func TestEnsureParentInsideCreatesMissingParent(t *testing.T) {
	// Production passes an EvalSymlinks-canonicalized root (dstAbs), so the
	// test must too — otherwise macOS's /var -> /private/var symlink trips
	// the escape guard on a perfectly nested path.
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	target := filepath.Join(root, "newdir", "deep", "file.txt")
	if err := ensureParentInside(root, target); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(root, "newdir", "deep")); err != nil {
		t.Fatalf("parent not created: %v", err)
	}
}

func TestEnsureParentInsideAcceptsNestedPath(t *testing.T) {
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	nested := filepath.Join(root, "a", "b")
	if err := os.MkdirAll(nested, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := ensureParentInside(root, filepath.Join(nested, "x.md")); err != nil {
		t.Fatalf("nested path refused: %v", err)
	}
}

func TestRestoreFailsWhenTmpCreationBlocked(t *testing.T) {
	parent := t.TempDir()
	dir := filepath.Join(parent, "repo")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	if _, err := Init(dir); err != nil {
		t.Fatal(err)
	}
	// A write-protected parent blocks MkdirTemp for the staging copy.
	if err := os.Chmod(parent, 0o555); err != nil {
		t.Fatal(err)
	}
	defer os.Chmod(parent, 0o755)
	src := t.TempDir()
	if err := os.WriteFile(filepath.Join(src, "x.txt"), []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := Restore(dir, src, "restore"); err == nil {
		t.Fatal("expected MkdirTemp failure")
	}
}

func TestRestoreFailsWhenWorkingTreeUnreadable(t *testing.T) {
	dir := t.TempDir()
	if _, err := Init(dir); err != nil {
		t.Fatal(err)
	}
	src := t.TempDir()
	if err := os.WriteFile(filepath.Join(src, "x.txt"), []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
	// After IsRepo, make the working tree unreadable so the entry sweep
	// (os.ReadDir) fails before any rename happens.
	if err := os.Chmod(dir, 0o000); err != nil {
		t.Fatal(err)
	}
	defer os.Chmod(dir, 0o755)
	if _, err := Restore(dir, src, "restore"); err == nil {
		t.Fatal("expected readdir failure")
	}
}

func TestRestoreFailsWhenEntryRemovalBlocked(t *testing.T) {
	dir := t.TempDir()
	if _, err := Init(dir); err != nil {
		t.Fatal(err)
	}
	if _, err := Commit(dir, "initial"); err != nil {
		t.Fatal(err)
	}
	blocked := filepath.Join(dir, "blocked")
	if err := os.MkdirAll(filepath.Join(blocked, "inner"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(blocked, "inner", "f.txt"), []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
	// The entry sweep removes every non-.git entry; a write-protected
	// directory cannot be removed recursively.
	if err := os.Chmod(blocked, 0o555); err != nil {
		t.Fatal(err)
	}
	defer os.Chmod(blocked, 0o755)
	src := t.TempDir()
	if err := os.WriteFile(filepath.Join(src, "x.txt"), []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := Restore(dir, src, "restore"); err == nil {
		t.Fatal("expected removal failure")
	}
}

func TestEnsureParentInsideFailsOnBrokenSymlink(t *testing.T) {
	root := t.TempDir()
	broken := filepath.Join(root, "broken")
	if err := os.Symlink(filepath.Join(root, "missing-target"), broken); err != nil {
		t.Fatal(err)
	}
	if err := ensureParentInside(root, filepath.Join(broken, "f.txt")); err == nil {
		t.Fatal("expected EvalSymlinks failure for broken parent symlink")
	}
}

func TestEnsureParentInsideFailsWhenParentPathBlocked(t *testing.T) {
	root := t.TempDir()
	// A regular file in the parent path makes MkdirAll fail with ENOTDIR.
	file := filepath.Join(root, "notdir")
	if err := os.WriteFile(file, []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := ensureParentInside(root, filepath.Join(file, "sub", "f.txt")); err == nil {
		t.Fatal("expected MkdirAll failure")
	}
}
