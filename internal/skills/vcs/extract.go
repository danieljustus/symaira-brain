package vcs

import (
	"archive/tar"
	"bytes"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

func ExtractRev(dir, rev, dst string) error {
	path, err := exec.LookPath("git")
	if err != nil {
		return ErrUnavailable
	}
	if err := os.MkdirAll(dst, 0o755); err != nil {
		return err
	}
	// Root confines every filesystem operation, including those following
	// symlinks already present in the destination or earlier archive entries.
	root, err := os.OpenRoot(dst)
	if err != nil {
		return err
	}
	defer root.Close()
	cmd := exec.Command(path, "archive", "--format=tar", rev)
	cmd.Dir = dir
	cmd.Env = gitEnv()
	var buf, errBuf bytes.Buffer
	cmd.Stdout = &buf
	cmd.Stderr = &errBuf
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("git archive %s: %w: %s", rev, err, strings.TrimSpace(errBuf.String()))
	}
	tr := tar.NewReader(&buf)
	for {
		hdr, err := tr.Next()
		if errors.Is(err, io.EOF) {
			return nil
		}
		if err != nil {
			return err
		}
		name := filepath.Clean(filepath.FromSlash(hdr.Name))
		if !filepath.IsLocal(name) || (name == "." && hdr.Typeflag != tar.TypeDir) {
			return fmt.Errorf("archive entry %q escapes destination", hdr.Name)
		}
		switch hdr.Typeflag {
		case tar.TypeDir:
			if err := root.MkdirAll(name, 0o755); err != nil {
				return err
			}
		case tar.TypeReg:
			if err := root.MkdirAll(filepath.Dir(name), 0o755); err != nil {
				return err
			}
			f, err := root.OpenFile(name, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, os.FileMode(hdr.Mode)&0o777)
			if err != nil {
				return err
			}
			if _, err := io.Copy(f, tr); err != nil {
				_ = f.Close()
				return err
			}
			if err := f.Close(); err != nil {
				return err
			}
		case tar.TypeSymlink:
			linkName := filepath.FromSlash(hdr.Linkname)
			linkPath := filepath.Clean(filepath.Join(filepath.Dir(name), linkName))
			if hdr.Linkname == "" || filepath.IsAbs(linkName) || !filepath.IsLocal(linkPath) || linkPath == "." {
				return fmt.Errorf("archive symlink %q escapes destination", hdr.Name)
			}
			if err := root.MkdirAll(filepath.Dir(name), 0o755); err != nil {
				return err
			}
			if err := root.Remove(name); err != nil && !os.IsNotExist(err) {
				return err
			}
			if err := root.Symlink(linkName, name); err != nil {
				return err
			}
		default:
			// Skip hardlinks, devices and other exotic entry types: the
			// tracked skill trees never contain them.
		}
	}
}

// Restore replaces the working tree of the repository at dir with a copy
// of the directory src (preserving .git) and records the change as a
// forward commit with the given message. History is never rewritten:
// Restore only ever adds a commit on top of HEAD, exactly like Commit.
// The copy happens in a temporary sibling first, so a failure never
// leaves dir half-written. Returns the full hash of the new commit, or ""
// when the resulting tree is identical to HEAD (nothing to commit).
