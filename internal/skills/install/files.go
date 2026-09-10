package install

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-brain/internal/skills/fsutil"
)

func copyDir(src, dst string) error {
	return fsutil.CopyTree(src, dst, func(rel string, d os.DirEntry) bool { return false })
}

// stripExecutableBits removes the executable bits from every regular file
// under root and returns the mode changes. With dryRun set it only reports
// the changes that would be applied. Symlinks are left untouched (they are
// resolved to regular copies by CopyTree, which then carry the target mode).
func stripExecutableBits(root string, dryRun bool) ([]ResourceModeChange, error) {
	var changes []ResourceModeChange
	err := filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() || d.Type()&os.ModeSymlink != 0 {
			return nil
		}
		info, err := d.Info()
		if err != nil {
			return err
		}
		perm := info.Mode().Perm()
		if perm&0o111 == 0 {
			return nil
		}
		stripped := perm &^ 0o111
		rel, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		changes = append(changes, ResourceModeChange{
			Path: filepath.ToSlash(rel),
			From: fmt.Sprintf("%04o", perm),
			To:   fmt.Sprintf("%04o", stripped),
		})
		if !dryRun {
			if err := os.Chmod(path, stripped); err != nil {
				return fmt.Errorf("strip executable bit from %s: %w", path, err)
			}
		}
		return nil
	})
	return changes, err
}
