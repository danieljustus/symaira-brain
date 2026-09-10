package skill

import (
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
)

// CopyBundleSupport copies materialized non-control bundle resources through
// the retained root capability. Symlinked files are copied as their resolved
// regular contents; overlays are render inputs and are never installed.
func CopyBundleSupport(bundle *Bundle, dst string) error {
	if bundle == nil {
		return fmt.Errorf("bundle is nil")
	}
	if err := os.MkdirAll(filepath.Dir(dst), 0o755); err != nil {
		return fmt.Errorf("create bundle destination parent: %w", err)
	}
	dstRoot, err := os.OpenRoot(filepath.Dir(dst))
	if err != nil {
		return fmt.Errorf("open bundle destination: %w", err)
	}
	defer dstRoot.Close()
	return CopyBundleSupportRoot(bundle, dstRoot, filepath.Base(dst))
}

// CopyBundleSupportRoot copies support files below dstRel in dstRoot. The
// caller owns the destination capability so every parent and file write stays
// confined even when the destination tree is modified concurrently.
func CopyBundleSupportRoot(bundle *Bundle, dstRoot *os.Root, dstRel string) error {
	if bundle == nil {
		return fmt.Errorf("bundle is nil")
	}
	if dstRoot == nil {
		return fmt.Errorf("destination has no root capability")
	}
	if err := EnsureRootPath(dstRoot, dstRel); err != nil {
		return err
	}
	for _, resource := range bundle.Resources {
		if resource.Path == "SKILL.md" || resource.Path == "symskills.toml" || isOverlayPath(resource.Path) {
			continue
		}
		data, err := ReadBundleBytes(bundle, resource.Path, MaxResourceSize)
		if err != nil {
			return fmt.Errorf("copy %s: %w", resource.Path, err)
		}
		target := filepath.ToSlash(filepath.Join(dstRel, filepath.FromSlash(resource.Path)))
		mode, err := strconv.ParseUint(strings.TrimSpace(resource.Mode), 8, 32)
		if err != nil {
			mode = 0o644
		}
		if err := WriteRootFile(dstRoot, target, data, os.FileMode(mode)&0o777); err != nil {
			return fmt.Errorf("copy %s: %w", resource.Path, err)
		}
	}
	return nil
}
