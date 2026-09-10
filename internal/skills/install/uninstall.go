package install

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"time"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

// Uninstall removes a managed installation and its base snapshot as one
// recoverable transaction. A tombstone is published only after all old state
// has been moved aside; failures restore every moved entry.
func Uninstall(target render.Target, name string, opts Options) (removed bool, err error) {
	if opts.Scope == "" {
		opts.Scope = render.ScopeUser
	}
	dest, pathErr := InstallPath(target, name, opts)
	if pathErr != nil {
		recordUninstallEvent(target, name, opts, "", pathErr)
		return false, pathErr
	}
	defer func() { recordUninstallEvent(target, name, opts, dest, err) }()

	if _, err := os.Lstat(dest); errors.Is(err, os.ErrNotExist) {
		return false, nil
	} else if err != nil {
		return false, fmt.Errorf("stat install destination: %w", err)
	}
	if err := ensureManagedMarker(dest); err != nil {
		return false, err
	}
	// A dry run must not create lockfiles, parents, tombstones, backups, or
	// events. Validation above is read-only and deliberately happens first.
	if opts.DryRun {
		return true, nil
	}

	lock, err := AcquirePullLock(target, name, PullOptions{HomeDir: opts.HomeDir})
	if err != nil {
		return false, err
	}
	defer func() { _ = lock.Release() }()

	base, err := BasePath(target, name, opts)
	if err != nil {
		return false, err
	}
	basePaths := []string{base}
	if opts.Scope == render.ScopeProject && opts.ProjectDir != "" {
		legacy := opts
		legacy.ProjectDir = ""
		legacyBase, lerr := BasePath(target, name, legacy)
		if lerr != nil {
			return false, lerr
		}
		if legacyBase != base {
			basePaths = append(basePaths, legacyBase)
		}
	}
	tombstone, err := TombstonePath(target, name, opts)
	if err != nil {
		return false, err
	}

	moved := make([]movedEntry, 0, len(basePaths)+2)
	rollback := func() {
		_ = os.Remove(tombstone)
		for i := len(moved) - 1; i >= 0; i-- {
			entry := moved[i]
			_ = os.RemoveAll(entry.original)
			_ = osRename(entry.backup, entry.original)
		}
	}
	move := func(path, prefix string) error {
		if _, statErr := os.Lstat(path); errors.Is(statErr, os.ErrNotExist) {
			return nil
		} else if statErr != nil {
			return statErr
		}
		backup, backupErr := transactionSibling(path, prefix)
		if backupErr != nil {
			return backupErr
		}
		if err := osRename(path, backup); err != nil {
			return fmt.Errorf("move %s aside: %w", path, err)
		}
		moved = append(moved, movedEntry{original: path, backup: backup})
		return nil
	}
	if err := move(dest, ".symskills-uninstall-dest-"); err != nil {
		return false, err
	}
	for _, path := range basePaths {
		if err := move(path, ".symskills-uninstall-base-"); err != nil {
			rollback()
			return false, err
		}
	}
	if err := move(tombstone, ".symskills-uninstall-tombstone-"); err != nil {
		rollback()
		return false, err
	}
	if err := writeTombstone(tombstone, target, name); err != nil {
		rollback()
		return false, err
	}
	for _, entry := range moved {
		if err := os.RemoveAll(entry.backup); err != nil {
			// The new state is still valid; retain the old backup rather than
			// risking a partially restored uninstall after publication.
			return false, fmt.Errorf("remove uninstall backup: %w", err)
		}
	}
	return true, nil
}

type movedEntry struct {
	original string
	backup   string
}

func transactionSibling(path, prefix string) (string, error) {
	parent := filepath.Dir(path)
	base := fmt.Sprintf("%s%s%d", prefix, filepath.Base(path), time.Now().UnixNano())
	candidate := filepath.Join(parent, base)
	for i := 0; i < 1024; i++ {
		info, err := os.Lstat(candidate)
		if errors.Is(err, os.ErrNotExist) {
			return candidate, nil
		}
		if err != nil {
			return "", err
		}
		_ = info
		candidate = fmt.Sprintf("%s-%d", filepath.Join(parent, base), i+1)
	}
	return "", fmt.Errorf("unable to allocate uninstall transaction path for %s", path)
}

func ensureManagedMarker(path string) error {
	resolved := path
	if info, err := os.Lstat(path); err == nil && info.Mode()&os.ModeSymlink != 0 {
		link, err := os.Readlink(path)
		if err != nil {
			return err
		}
		if !filepath.IsAbs(link) {
			link = filepath.Join(filepath.Dir(path), link)
		}
		if _, err := os.Stat(link); errors.Is(err, os.ErrNotExist) {
			// A dangling symlink is a managed install whose rendered cache
			// entry was already removed. Remove only the link itself.
			return nil
		}
		resolved = link
	}
	marker, managed, err := readInstallMarker(resolved)
	if err != nil {
		return err
	}
	if !managed {
		return fmt.Errorf("refusing to remove unmanaged skill at %s", path)
	}
	if marker.ManagedBy == "" && marker.Target == "" && marker.Name == "" && marker.Mode == "" && marker.SourceHash != "" {
		return fmt.Errorf("refusing to remove unmanaged skill at %s", path)
	}
	if marker.ManagedBy != "" && marker.ManagedBy != "symskills" {
		return fmt.Errorf("refusing to remove unmanaged skill at %s", path)
	}
	if marker.ManagedBy == "symskills" && (marker.Target == "" || marker.Name == "" || marker.Name != filepath.Base(path)) {
		return fmt.Errorf("refusing to remove unmanaged skill at %s", path)
	}
	if marker.Mode != "" && marker.Mode != ModeCopy && marker.Mode != ModeSymlink {
		return fmt.Errorf("refusing to remove unmanaged skill at %s", path)
	}
	return checkMarkerWritable(filepath.Join(resolved, markerFile))
}

func writeTombstone(path string, target render.Target, name string) error {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	data, err := json.MarshalIndent(map[string]any{
		"schema_version": BaseSchemaVersion,
		"target":         string(target),
		"name":           name,
		"removed_at":     time.Now().UTC().Format(time.RFC3339),
	}, "", "  ")
	if err != nil {
		return err
	}
	data = append(data, '\n')
	tmp, err := transactionSibling(path, ".symskills-tombstone-")
	if err != nil {
		return err
	}
	if err := os.WriteFile(tmp, data, 0o644); err != nil {
		return err
	}
	if err := osRename(tmp, path); err != nil {
		_ = os.Remove(tmp)
		return fmt.Errorf("publish base tombstone: %w", err)
	}
	return nil
}

// Diff compares a freshly rendered skill against its installed state.
