//go:build darwin || dragonfly || freebsd || linux || netbsd || openbsd || solaris

package instructions

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"golang.org/x/sys/unix"
)

func TestSourceRejectsSymlinkedProjectAndHomeParents(t *testing.T) {
	root := t.TempDir()
	outside := t.TempDir()
	projectLink := filepath.Join(root, "project-link")
	if err := os.Symlink(outside, projectLink); err != nil {
		t.Fatal(err)
	}
	projectPath := filepath.Join(projectLink, ProjectDirName, ProjectFileName)
	if _, err := FromPaths(filepath.Join(root, "missing-global", GlobalFileName), projectPath).Content(); err == nil {
		t.Fatal("source followed a symlinked project parent")
	}

	homeReal := filepath.Join(root, "home-real")
	if err := os.MkdirAll(filepath.Join(homeReal, ".config", "symbrain"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(homeReal, ".config", "symbrain", GlobalFileName), []byte("outside"), 0o600); err != nil {
		t.Fatal(err)
	}
	homeLink := filepath.Join(root, "home-link")
	if err := os.Symlink(homeReal, homeLink); err != nil {
		t.Fatal(err)
	}
	t.Setenv("HOME", homeLink)
	t.Setenv("XDG_CONFIG_HOME", "")
	if _, err := NewSource("").Content(); err == nil {
		t.Fatal("source followed a symlinked HOME parent")
	}
}

func TestSourceNeverReopensMissingParentByPath(t *testing.T) {
	root := t.TempDir()
	outside := t.TempDir()
	missingPath := filepath.Join(root, "late", "instructions.md")
	source := FromPaths(missingPath, "")
	if err := os.WriteFile(filepath.Join(outside, "instructions.md"), []byte("outside"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(root, "late")); err != nil {
		t.Fatal(err)
	}
	content, err := source.Content()
	if err != nil {
		t.Fatal(err)
	}
	if content != "" {
		t.Fatalf("late symlink content = %q, want empty", content)
	}
}

func TestSourceFIFOReplacementReturnsWithoutBlocking(t *testing.T) {
	root := t.TempDir()
	path := filepath.Join(root, "instructions.md")
	if err := os.WriteFile(path, []byte("regular"), 0o600); err != nil {
		t.Fatal(err)
	}
	source := FromPaths(path, "")
	if err := os.Remove(path); err != nil {
		t.Fatal(err)
	}
	if err := unix.Mkfifo(path, 0o600); err != nil {
		t.Fatal(err)
	}
	done := make(chan error, 1)
	go func() {
		_, err := source.Content()
		done <- err
	}()
	select {
	case err := <-done:
		if err == nil {
			t.Fatal("FIFO replacement was accepted")
		}
	case <-time.After(time.Second):
		t.Fatal("FIFO replacement blocked source read")
	}
}
