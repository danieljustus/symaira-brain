package fsutil

import (
	"os"
	"path/filepath"
	"testing"
)

func TestCopyTreeInternalSymlinkToDirectoryRecurses(t *testing.T) {
	src := t.TempDir()
	writeFile(t, filepath.Join(src, "real", "inner.txt"), "content")
	if err := os.Symlink(filepath.Join(src, "real"), filepath.Join(src, "link")); err != nil {
		t.Fatal(err)
	}

	dst := t.TempDir()
	if err := CopyTree(src, dst, func(rel string, d os.DirEntry) bool { return false }); err != nil {
		t.Fatalf("CopyTree: %v", err)
	}

	if _, err := os.Stat(filepath.Join(dst, "link", "inner.txt")); err != nil {
		t.Fatalf("link/inner.txt missing after recursing through internal directory symlink: %v", err)
	}
}

func TestCopyTreeRejectsDanglingSymlink(t *testing.T) {
	src := t.TempDir()
	if err := os.Symlink(filepath.Join(src, "does-not-exist"), filepath.Join(src, "broken")); err != nil {
		t.Fatal(err)
	}

	dst := t.TempDir()
	err := CopyTree(src, dst, func(rel string, d os.DirEntry) bool { return false })
	if err == nil {
		t.Fatal("expected error for dangling symlink, got nil")
	}
}

func TestCopyFileFailsForMissingSource(t *testing.T) {
	dst := filepath.Join(t.TempDir(), "out.txt")
	err := CopyFile(filepath.Join(t.TempDir(), "missing.txt"), dst, 0o644)
	if err == nil {
		t.Fatal("expected error copying a nonexistent source file")
	}
}

func TestCopyFileFailsWhenDestDirBlockedByFile(t *testing.T) {
	root := t.TempDir()
	blocker := filepath.Join(root, "blocker")
	writeFile(t, blocker, "not a directory")

	src := filepath.Join(t.TempDir(), "src.txt")
	writeFile(t, src, "content")

	// dst's parent directory path passes through a regular file, so
	// MkdirAll(filepath.Dir(dst)) must fail.
	dst := filepath.Join(blocker, "nested", "out.txt")
	err := CopyFile(src, dst, 0o644)
	if err == nil {
		t.Fatal("expected error when destination directory path is blocked by a file")
	}
}
