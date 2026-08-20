package fsutil

import (
	"os"
	"path/filepath"
	"testing"
)

func TestCopyTreeCopiesFilesAndRespectsSkip(t *testing.T) {
	src := t.TempDir()
	writeFile(t, filepath.Join(src, "keep.txt"), "keep")
	writeFile(t, filepath.Join(src, "skip.txt"), "skip")
	writeFile(t, filepath.Join(src, "nested", "deep.txt"), "deep")

	dst := t.TempDir()
	if err := CopyTree(src, dst, func(rel string, d os.DirEntry) bool {
		return rel == "skip.txt"
	}); err != nil {
		t.Fatalf("CopyTree: %v", err)
	}

	if _, err := os.Stat(filepath.Join(dst, "keep.txt")); err != nil {
		t.Fatalf("keep.txt missing: %v", err)
	}
	if _, err := os.Stat(filepath.Join(dst, "nested", "deep.txt")); err != nil {
		t.Fatalf("nested/deep.txt missing: %v", err)
	}
	if _, err := os.Stat(filepath.Join(dst, "skip.txt")); !os.IsNotExist(err) {
		t.Fatalf("skip.txt should have been skipped: %v", err)
	}
}

func TestCopyTreeSkipsDirectoryRecursively(t *testing.T) {
	src := t.TempDir()
	writeFile(t, filepath.Join(src, "a", "keep.txt"), "keep")
	writeFile(t, filepath.Join(src, "b", "skip.txt"), "skip")

	dst := t.TempDir()
	if err := CopyTree(src, dst, func(rel string, d os.DirEntry) bool {
		return d.Name() == "b" && d.IsDir()
	}); err != nil {
		t.Fatalf("CopyTree: %v", err)
	}

	if _, err := os.Stat(filepath.Join(dst, "a", "keep.txt")); err != nil {
		t.Fatalf("a/keep.txt missing: %v", err)
	}
	if _, err := os.Stat(filepath.Join(dst, "b")); !os.IsNotExist(err) {
		t.Fatalf("b directory should have been skipped: %v", err)
	}
}

func TestCopyTreeRejectsSymlinkEscapingSourceRoot(t *testing.T) {
	outside := t.TempDir()
	writeFile(t, filepath.Join(outside, "secret.txt"), "secret")

	src := t.TempDir()
	if err := os.Symlink(filepath.Join(outside, "secret.txt"), filepath.Join(src, "leak.txt")); err != nil {
		t.Fatal(err)
	}

	dst := t.TempDir()
	err := CopyTree(src, dst, func(rel string, d os.DirEntry) bool { return false })
	if err == nil {
		t.Fatal("expected error for symlink escaping source root, got nil")
	}
}

func TestCopyTreeAllowsInternalSymlink(t *testing.T) {
	src := t.TempDir()
	writeFile(t, filepath.Join(src, "real.txt"), "content")
	if err := os.Symlink(filepath.Join(src, "real.txt"), filepath.Join(src, "sym.txt")); err != nil {
		t.Fatal(err)
	}

	dst := t.TempDir()
	if err := CopyTree(src, dst, func(rel string, d os.DirEntry) bool { return false }); err != nil {
		t.Fatalf("CopyTree: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(dst, "sym.txt"))
	if err != nil {
		t.Fatalf("ReadFile sym.txt: %v", err)
	}
	if string(data) != "content" {
		t.Fatalf("unexpected content: %q", string(data))
	}
}

func writeFile(t *testing.T, path, content string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
}
