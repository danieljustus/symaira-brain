// Package fsutil provides small, testable filesystem helpers used across symskills.
package fsutil

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

// CopyTree copies the directory tree at src to dst. For each file and
// directory the skip predicate is called with the path relative to src and
// the directory entry. If skip returns true the entry is omitted; directories
// are skipped recursively.
func CopyTree(src, dst string, skip func(rel string, d os.DirEntry) bool) error {
	realSrc, err := filepath.EvalSymlinks(src)
	if err != nil {
		realSrc = src
	}
	realSrc, err = filepath.Abs(realSrc)
	if err != nil {
		realSrc = src
	}

	return filepath.WalkDir(src, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(src, path)
		if err != nil {
			return fmt.Errorf("rel %q: %w", path, err)
		}
		if rel == "." {
			return os.MkdirAll(dst, 0o755)
		}
		if skip(rel, d) {
			if d.IsDir() {
				return filepath.SkipDir
			}
			return nil
		}
		target := filepath.Join(dst, rel)
		if d.Type()&os.ModeSymlink != 0 {
			realPath, err := filepath.EvalSymlinks(path)
			if err != nil {
				return fmt.Errorf("eval symlink %q: %w", path, err)
			}
			realPath, err = filepath.Abs(realPath)
			if err != nil {
				return fmt.Errorf("abs path %q: %w", realPath, err)
			}
			relTarget, err := filepath.Rel(realSrc, realPath)
			if err != nil || relTarget == ".." || strings.HasPrefix(relTarget, ".."+string(filepath.Separator)) {
				return fmt.Errorf("symlink %q escapes source root %q: points to %q", rel, src, realPath)
			}
			info, err := os.Stat(realPath)
			if err != nil {
				return err
			}
			if info.IsDir() {
				return CopyTree(realPath, target, skip)
			}
			return CopyFile(realPath, target, info.Mode().Perm())
		}
		if d.IsDir() {
			return os.MkdirAll(target, 0o755)
		}
		info, err := d.Info()
		if err != nil {
			return err
		}
		return CopyFile(path, target, info.Mode().Perm())
	})
}

// CopyFile copies the contents of src to dst, creating dst with the given
// permission bits if it does not exist or truncating it if it does.
func CopyFile(src, dst string, perm os.FileMode) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()
	if err := os.MkdirAll(filepath.Dir(dst), 0o755); err != nil {
		return err
	}
	out, err := os.OpenFile(dst, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, perm)
	if err != nil {
		return err
	}
	if _, err := io.Copy(out, in); err != nil {
		_ = out.Close()
		return err
	}
	return out.Close()
}
