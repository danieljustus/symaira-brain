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
	// Resolve the destination root so later EvalSymlinks comparisons agree
	// on the canonical path (on macOS /var resolves to /private/var).
	dstAbs, err := filepath.EvalSymlinks(dst)
	if err != nil {
		return err
	}
	cmd := exec.Command(path, "archive", "--format=tar", rev)
	cmd.Dir = dir
	cmd.Env = gitEnv()
	var buf, errBuf bytes.Buffer
	cmd.Stdout = &buf
	cmd.Stderr = &errBuf
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("git archive %s: %w: %s", rev, err, strings.TrimSpace(errBuf.String()))
	}
	dstAbs = filepath.Clean(dstAbs)
	tr := tar.NewReader(&buf)
	for {
		// Extraction is guarded by layered, regression-tested checks
		// (prefix check on the joined target, EvalSymlinks parent
		// resolution, absolute/dotdot linkname refusal) which CodeQL's
		// taint model does not recognize.
		// CodeQL: exclude — target is confined below dstAbs and its
		// resolved parent is checked before every extraction operation.
		hdr, err := tr.Next()
		if errors.Is(err, io.EOF) {
			return nil
		}
		if err != nil {
			return err
		}
		name := filepath.Clean(filepath.FromSlash(hdr.Name))
		target := filepath.Join(dstAbs, name)
		// Canonical zip-slip guard on the joined target: it must stay
		// under the destination root.
		if target != dstAbs && !strings.HasPrefix(target, filepath.Clean(dstAbs)+string(os.PathSeparator)) {
			return fmt.Errorf("archive entry %q escapes destination", hdr.Name)
		}
		// Verify the (possibly symlinked) parent still resolves inside the
		// destination root. Without this, an earlier symlink entry could
		// redirect a later file write outside the archive root.
		if err := ensureParentInside(dstAbs, target); err != nil {
			return err
		}
		switch hdr.Typeflag {
		case tar.TypeDir:
			if err := os.MkdirAll(target, 0o755); err != nil {
				return err
			}
		case tar.TypeReg:
			if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
				return err
			}
			f, err := os.OpenFile(target, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, os.FileMode(hdr.Mode)&0o777)
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
			// The link name must not be absolute and, once resolved
			// against the link's directory, must stay inside the
			// destination root — an unchecked linkname could point
			// anywhere on the machine.
			linkPath := filepath.Clean(filepath.Join(filepath.Dir(target), filepath.FromSlash(hdr.Linkname)))
			if filepath.IsAbs(hdr.Linkname) || !strings.HasPrefix(linkPath, filepath.Clean(dstAbs)+string(os.PathSeparator)) {
				return fmt.Errorf("archive symlink %q escapes destination", hdr.Name)
			}
			_ = os.Remove(target)
			// CodeQL: exclude — absolute and escaping link targets are
			// rejected before the symlink is created.
			if err := os.Symlink(hdr.Linkname, target); err != nil {
				return err
			}
		default:
			// Skip hardlinks, devices and other exotic entry types: the
			// tracked skill trees never contain them.
		}
	}
}

// ensureParentInside verifies that the parent directory of path resolves
// inside root, so writes cannot be redirected through a symlink created by
// an earlier archive entry. The parent is created first when missing.
func ensureParentInside(root, path string) error {
	parent := filepath.Dir(path)
	if err := os.MkdirAll(parent, 0o755); err != nil {
		return err
	}
	resolved, err := filepath.EvalSymlinks(parent)
	if err != nil {
		return err
	}
	rel, err := filepath.Rel(root, resolved)
	if err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return fmt.Errorf("archive entry escapes destination via symlink: %q resolves to %q", path, resolved)
	}
	return nil
}

// Restore replaces the working tree of the repository at dir with a copy
// of the directory src (preserving .git) and records the change as a
// forward commit with the given message. History is never rewritten:
// Restore only ever adds a commit on top of HEAD, exactly like Commit.
// The copy happens in a temporary sibling first, so a failure never
// leaves dir half-written. Returns the full hash of the new commit, or ""
// when the resulting tree is identical to HEAD (nothing to commit).
