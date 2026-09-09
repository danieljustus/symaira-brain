package vcs

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-brain/internal/skills/fsutil"
)

func Restore(dir, src, message string) (string, error) {
	if !IsRepo(dir) {
		return "", fmt.Errorf("not a git repository: %s", dir)
	}
	tmp, err := os.MkdirTemp(filepath.Dir(dir), ".restore-tmp-*")
	if err != nil {
		return "", err
	}
	defer os.RemoveAll(tmp)
	if err := fsutil.CopyTree(src, tmp, func(_ string, d os.DirEntry) bool {
		return d.Name() == ".git" && d.IsDir()
	}); err != nil {
		return "", err
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		return "", err
	}
	for _, entry := range entries {
		if entry.Name() == ".git" {
			continue
		}
		if err := os.RemoveAll(filepath.Join(dir, entry.Name())); err != nil {
			return "", err
		}
	}
	tmpEntries, err := os.ReadDir(tmp)
	if err != nil {
		return "", err
	}
	for _, entry := range tmpEntries {
		if err := os.Rename(filepath.Join(tmp, entry.Name()), filepath.Join(dir, entry.Name())); err != nil {
			return "", err
		}
	}
	return Commit(dir, message)
}
