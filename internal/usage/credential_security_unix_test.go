//go:build unix

package usage

import (
	"os"
	"path/filepath"
	"testing"
)

func TestReadCredentialFileRejectsSymlink(t *testing.T) {
	dir := t.TempDir()
	target := filepath.Join(dir, "target.json")
	link := filepath.Join(dir, "credential.json")
	if err := os.WriteFile(target, []byte(`{"token":"must-not-follow"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(target, link); err != nil {
		t.Fatal(err)
	}

	if _, err := readCredentialFile(link); err == nil {
		t.Fatal("readCredentialFile followed a symlink")
	}
}
