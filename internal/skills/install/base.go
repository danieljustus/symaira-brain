package install

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"time"

	"github.com/danieljustus/symaira-brain/internal/skills/fsutil"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
	corefsutil "github.com/danieljustus/symaira-corekit/fsutil"
)

// manifestFile is the name of the base snapshot manifest written next to the
// frozen file set. Each entry records the file's sha256 and permission bits.
const manifestFile = "manifest.json"

// BaseSchemaVersion is the manifest.json format version this build writes.
const BaseSchemaVersion = 1

// ErrNotManaged reports that a skill has no persisted base snapshot, which
// means it was never installed by symskills (or was installed before base
// snapshots existed). It is deliberately distinct from "deleted": uninstall
// leaves a tombstone, so a missing base can only mean "never managed".
var ErrNotManaged = errors.New("skill is not managed by symskills (no base snapshot)")

// BaseFileEntry describes one file in a base snapshot.
type BaseFileEntry struct {
	SHA256 string `json:"sha256"`
	Mode   string `json:"mode"`
}

// BaseManifest is the manifest.json shape written alongside a base snapshot.
// Files is keyed by the slash-separated path relative to the snapshot root.
type BaseManifest struct {
	SchemaVersion int                      `json:"schema_version"`
	Target        string                   `json:"target"`
	Name          string                   `json:"name"`
	Files         map[string]BaseFileEntry `json:"files"`
}

// BasePath returns the base snapshot directory for a skill. The default
// location is ~/.local/share/symskills/base/<target>/<name>; project-scope
// installs are kept under base/<target>/project/<name> so the two scopes
// cannot clobber each other.
func BasePath(target render.Target, name string, opts Options) (string, error) {
	base := opts.BaseDir
	if base == "" {
		home := opts.HomeDir
		if home == "" {
			var err error
			home, err = os.UserHomeDir()
			if err != nil {
				return "", err
			}
		}
		base = filepath.Join(home, ".local", "share", "symskills", "base")
	}
	if opts.Scope == render.ScopeProject {
		return filepath.Join(base, string(target), string(opts.Scope), name), nil
	}
	return filepath.Join(base, string(target), name), nil
}

// TombstonePath returns the tombstone marker written when a managed skill is
// uninstalled. Its presence distinguishes "deleted" from "never managed".
func TombstonePath(target render.Target, name string, opts Options) (string, error) {
	base, err := BasePath(target, name, opts)
	if err != nil {
		return "", err
	}
	return base + ".tombstone", nil
}

// WriteBaseSnapshot freezes the rendered tree at src into the base directory
// for target/name: every file (marker excluded) plus a manifest.json with a
// sha256 and the permission bits per file. The snapshot is staged in a
// sibling directory and swapped into place with renames, so an interrupted
// write leaves either the old or the new base, never a partial one. A
// leftover tombstone from a previous uninstall is cleared on success.
func WriteBaseSnapshot(src string, target render.Target, name string, opts Options) error {
	baseDir, err := BasePath(target, name, opts)
	if err != nil {
		return err
	}
	tmp := baseDir + ".tmp-" + strconv.Itoa(os.Getpid())
	if err := os.RemoveAll(tmp); err != nil {
		return fmt.Errorf("clearing stale base staging path: %w", err)
	}
	defer func() { _ = os.RemoveAll(tmp) }()
	if err := os.MkdirAll(tmp, 0o755); err != nil {
		return err
	}
	// The marker is install bookkeeping, not installed content; it never
	// enters the snapshot.
	if err := fsutil.CopyTree(src, tmp, func(rel string, d os.DirEntry) bool {
		return rel == markerFile
	}); err != nil {
		return fmt.Errorf("copying base snapshot: %w", err)
	}
	manifest, err := buildBaseManifest(tmp, target, name)
	if err != nil {
		return err
	}
	data, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return err
	}
	data = append(data, '\n')
	if err := corefsutil.SafeWriteFile(filepath.Join(tmp, manifestFile), data, 0o644); err != nil {
		return fmt.Errorf("writing base manifest: %w", err)
	}
	if err := swapBaseDir(baseDir, tmp); err != nil {
		return err
	}
	if t, err := TombstonePath(target, name, opts); err == nil {
		_ = os.Remove(t)
	}
	return nil
}

// buildBaseManifest computes the per-file digests and mode bits of the staged
// snapshot tree.
func buildBaseManifest(root string, target render.Target, name string) (BaseManifest, error) {
	manifest := BaseManifest{
		SchemaVersion: BaseSchemaVersion,
		Target:        string(target),
		Name:          name,
		Files:         map[string]BaseFileEntry{},
	}
	err := filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			return nil
		}
		rel, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		if rel == manifestFile {
			return nil
		}
		info, err := d.Info()
		if err != nil {
			return err
		}
		sum, err := hashFile(path)
		if err != nil {
			return err
		}
		manifest.Files[filepath.ToSlash(rel)] = BaseFileEntry{
			SHA256: sum,
			Mode:   fmt.Sprintf("%04o", info.Mode().Perm()),
		}
		return nil
	})
	if err != nil {
		return BaseManifest{}, fmt.Errorf("building base manifest: %w", err)
	}
	return manifest, nil
}

func hashFile(path string) (string, error) {
	f, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer f.Close()
	h := sha256.New()
	if _, err := io.Copy(h, f); err != nil {
		return "", err
	}
	return hex.EncodeToString(h.Sum(nil)), nil
}

// swapBaseDir atomically replaces baseDir with the staged tmp directory: the
// previous base is renamed aside, tmp is promoted, and the backup is removed.
// When the promotion fails the previous base is restored.
func swapBaseDir(baseDir, tmp string) error {
	bak := baseDir + ".bak"
	if err := os.RemoveAll(bak); err != nil {
		return err
	}
	moved := false
	if _, err := os.Lstat(baseDir); err == nil {
		if err := osRename(baseDir, bak); err != nil {
			return fmt.Errorf("moving previous base aside: %w", err)
		}
		moved = true
	}
	if err := osRename(tmp, baseDir); err != nil {
		if moved {
			if rerr := osRename(bak, baseDir); rerr != nil {
				return fmt.Errorf("publishing base snapshot: %w (restoring previous base: %v)", err, rerr)
			}
		}
		return fmt.Errorf("publishing base snapshot: %w", err)
	}
	_ = os.RemoveAll(bak)
	return nil
}

// RemoveBase deletes the base snapshot for a skill and leaves a tombstone so
// "deleted" stays distinguishable from "never managed".
func RemoveBase(target render.Target, name string, opts Options) error {
	baseDir, err := BasePath(target, name, opts)
	if err != nil {
		return err
	}
	if err := os.RemoveAll(baseDir); err != nil {
		return err
	}
	t, err := TombstonePath(target, name, opts)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(t), 0o755); err != nil {
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
	if err := corefsutil.SafeWriteFile(t, data, 0o644); err != nil {
		return fmt.Errorf("writing base tombstone: %w", err)
	}
	return nil
}
