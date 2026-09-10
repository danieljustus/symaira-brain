package install

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"time"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

// Install performs one rendered-skill installation and records an optional
// best-effort operation event. The implementation is kept separate so direct
// callers and MCP callers can choose whether to add their own richer event.
func Install(item RenderedSkill, opts Options) (Result, error) {
	result, err := install(item, opts)
	recordInstallEvent(item, opts, result, err)
	return result, err
}

func install(item RenderedSkill, opts Options) (Result, error) {
	if opts.Scope == "" {
		opts.Scope = render.ScopeUser
	}
	if opts.Mode == "" {
		opts.Mode = ModeSymlink
	}
	dest, err := InstallPath(item.Target, item.Name, opts)
	if err != nil {
		return Result{}, err
	}
	result := Result{Action: "installed", Target: item.Target, Name: item.Name, Path: dest, Mode: opts.Mode}
	// Executable-bit policy: by default strip the executable bit from every
	// resource before the tree is materialized (symlink or copy), so no
	// executable file is planted into a harness directory. The changes are
	// reported on the result; --allow-executable (or the manifest setting)
	// preserves them. In a dry run nothing is modified, but the planned
	// changes are still reported.
	if !opts.AllowExecutable {
		changes, err := stripExecutableBits(item.Path, opts.DryRun)
		if err != nil {
			return Result{}, fmt.Errorf("strip executable bits: %w", err)
		}
		result.ModeChanges = changes
	}
	// When the harness skills directory is itself a symlink into the render
	// cache, dest and the rendered source are the same location. Removing dest
	// would delete the very content we are about to link to, so treat this as
	// already installed and only refresh the marker.
	same, err := sameLocation(dest, item.Path)
	if err != nil {
		return Result{}, err
	}
	if same {
		result.Action = "current"
		if opts.DryRun {
			result.Action = "planned"
			return result, nil
		}
		if err := writeMarker(item.Path, item, opts.Mode, opts.AllowExecutable); err != nil {
			return Result{}, err
		}
		if err := WriteBaseSnapshot(item.Path, item.Target, item.Name, opts); err != nil {
			return Result{}, err
		}
		return result, nil
	}
	if opts.DryRun {
		result.Action = "planned"
		return result, nil
	}
	backup, err := prepareDest(dest, opts)
	if err != nil {
		return Result{}, err
	}
	result.BackupPath = backup
	if err := writeMarker(item.Path, item, opts.Mode, opts.AllowExecutable); err != nil {
		return Result{}, err
	}
	if err := installAtomic(item, dest, &opts, &result); err != nil {
		return Result{}, err
	}
	if err := WriteBaseSnapshot(item.Path, item.Target, item.Name, opts); err != nil {
		return Result{}, err
	}
	return result, nil
}

// osRename is a seam for fault-injection tests of the atomic swap sequence.
var osRename = os.Rename

// installAtomic materializes the rendered skill into a sibling dest.tmp-<pid>
// staging path and swaps it into place with two renames, so dest is never
// observed empty or half-written. The symlink fast path gets the same
// treatment: the link is created at the staging path and a failure falls
// back to a staged copy; dest is only ever replaced by the final rename. On
// any error the partial staging path is removed and the previous install
// (held at dest.bak) is restored.
func installAtomic(item RenderedSkill, dest string, opts *Options, result *Result) error {
	tmp := dest + ".tmp-" + strconv.Itoa(os.Getpid())
	if err := os.RemoveAll(tmp); err != nil {
		return fmt.Errorf("clearing stale staging path: %w", err)
	}
	if err := os.MkdirAll(filepath.Dir(dest), 0o755); err != nil {
		return err
	}
	if opts.Mode == ModeSymlink {
		if err := os.Symlink(item.Path, tmp); err != nil {
			// Symlinks are unavailable here; fall back to a staged copy.
			opts.Mode = ModeCopy
			result.Mode = ModeCopy
		}
	}
	if opts.Mode == ModeCopy {
		if err := copyDir(item.Path, tmp); err != nil {
			_ = os.RemoveAll(tmp)
			return err
		}
	}
	if err := swapIntoPlace(dest, tmp); err != nil {
		_ = os.RemoveAll(tmp)
		return err
	}
	return nil
}

// swapIntoPlace atomically replaces dest with the materialized tmp staging
// path: dest is renamed aside to dest.bak, tmp is promoted to dest, and the
// backup is deleted. When the promotion fails, the previous install is
// restored from dest.bak.
func swapIntoPlace(dest, tmp string) error {
	bak := dest + ".bak"
	if err := os.RemoveAll(bak); err != nil {
		return err
	}
	moved := false
	if _, err := os.Lstat(dest); err == nil {
		if err := osRename(dest, bak); err != nil {
			return fmt.Errorf("moving previous install aside: %w", err)
		}
		moved = true
	}
	if err := osRename(tmp, dest); err != nil {
		if moved {
			if rerr := osRename(bak, dest); rerr != nil {
				return fmt.Errorf("publishing install: %w (restoring previous install: %v)", err, rerr)
			}
		}
		return fmt.Errorf("publishing install: %w", err)
	}
	_ = os.RemoveAll(bak)
	return nil
}

// sameLocation reports whether dest and src denote the same directory once
// symlinked path components are resolved. dest itself is deliberately not
// followed: a dest symlink pointing at src is a normal, already-done install,
// whereas a dest whose *parent* resolves into src's directory is the dangerous
// case this guards.
func sameLocation(dest, src string) (bool, error) {
	srcReal, err := filepath.EvalSymlinks(src)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return false, nil
		}
		return false, err
	}
	destParent, err := filepath.EvalSymlinks(filepath.Dir(dest))
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return false, nil
		}
		return false, err
	}
	return filepath.Join(destParent, filepath.Base(dest)) == srcReal, nil
}

// prepareDest clears the way for an install. Without opts.Force it only
// verifies that dest is absent or symskills-managed. With opts.Force an
// unmanaged dest is moved to a backup directory and its path returned.
func prepareDest(dest string, opts Options) (string, error) {
	err := ensureManagedOrAbsent(dest)
	if err == nil || !opts.Force {
		return "", err
	}
	st, lerr := os.Lstat(dest)
	if lerr != nil {
		return "", lerr
	}
	// A symlink holds no content of its own; drop it and leave its target alone.
	if st.Mode()&os.ModeSymlink != 0 {
		return "", os.Remove(dest)
	}
	backup, berr := backupPath(dest, opts)
	if berr != nil {
		return "", berr
	}
	if err := os.MkdirAll(filepath.Dir(backup), 0o755); err != nil {
		return "", err
	}
	if err := os.Rename(dest, backup); err != nil {
		return "", fmt.Errorf("backing up unmanaged skill at %s: %w", dest, err)
	}
	return backup, nil
}

// backupPath returns a collision-free location outside every harness directory,
// so the moved-aside skill is not picked up as a skill by any agent.
func backupPath(dest string, opts Options) (string, error) {
	home := opts.HomeDir
	if home == "" {
		var err error
		home, err = os.UserHomeDir()
		if err != nil {
			return "", err
		}
	}
	base := filepath.Join(home, ".local", "share", "symskills", "backups",
		fmt.Sprintf("%s-%s", filepath.Base(dest), time.Now().UTC().Format("20060102T150405Z")))
	path := base
	for i := 1; ; i++ {
		if _, err := os.Lstat(path); errors.Is(err, os.ErrNotExist) {
			return path, nil
		} else if err != nil {
			return "", err
		}
		path = fmt.Sprintf("%s-%d", base, i)
	}
}

func ensureManagedOrAbsent(path string) error {
	st, err := os.Lstat(path)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	} else if err != nil {
		return err
	}
	if st.Mode()&os.ModeSymlink != 0 {
		target, err := os.Readlink(path)
		if err != nil {
			return err
		}
		if !filepath.IsAbs(target) {
			target = filepath.Join(filepath.Dir(path), target)
		}
		if _, err := os.Stat(target); errors.Is(err, os.ErrNotExist) {
			return nil
		}
		path = target
	}
	marker, managed, err := readInstallMarker(path)
	if err != nil {
		return err
	}
	if !managed {
		return fmt.Errorf("refusing to overwrite unmanaged skill at %s", path)
	}
	if marker.ManagedBy == "" && marker.Target == "" && marker.Name == "" && marker.Mode == "" && marker.SourceHash != "" {
		return fmt.Errorf("refusing to overwrite unmanaged skill at %s", path)
	}
	if marker.ManagedBy != "" && marker.ManagedBy != "symskills" {
		return fmt.Errorf("refusing to overwrite unmanaged skill at %s", path)
	}
	if marker.ManagedBy == "symskills" && (marker.Target == "" || marker.Name == "" || marker.Name != filepath.Base(path)) {
		return fmt.Errorf("refusing to overwrite unmanaged skill at %s", path)
	}
	if marker.Mode != "" && marker.Mode != ModeCopy && marker.Mode != ModeSymlink {
		return fmt.Errorf("refusing to overwrite unmanaged skill at %s", path)
	}
	return checkMarkerWritable(filepath.Join(path, markerFile))
}

func markerBytes(item RenderedSkill, mode Mode, allowExecutable bool) []byte {
	// Preserve source_hash from any existing marker so the render-cache
	// freshness check survives install (#87).
	var srcHash string
	if data, err := os.ReadFile(filepath.Join(item.Path, markerFile)); err == nil {
		var existing struct {
			SourceHash string `json:"source_hash,omitempty"`
		}
		if json.Unmarshal(data, &existing) == nil {
			srcHash = existing.SourceHash
		}
	}
	m := Marker{
		SchemaVersion:   MarkerSchemaVersion,
		ManagedBy:       "symskills",
		Target:          item.Target,
		Name:            item.Name,
		RenderedAt:      item.Path,
		Mode:            mode,
		Installed:       time.Now().UTC().Format(time.RFC3339),
		SourceHash:      srcHash,
		AllowExecutable: allowExecutable,
	}
	data, _ := json.MarshalIndent(m, "", "  ")
	return append(data, '\n')
}

// markerSchemaVersion returns the schema_version of a serialized marker.
// Markers written before versioning existed carry no field; those are
// treated as version 1.
func markerSchemaVersion(data []byte) (int, error) {
	var m struct {
		SchemaVersion int `json:"schema_version"`
	}
	if err := json.Unmarshal(data, &m); err != nil {
		return 0, fmt.Errorf("parsing marker: %w", err)
	}
	if m.SchemaVersion == 0 {
		return MarkerSchemaVersion, nil
	}
	return m.SchemaVersion, nil
}

// checkMarkerWritable refuses to overwrite a marker written by a newer
// symskills or macOS client than this build understands.
func checkMarkerWritable(path string) error {
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	v, err := markerSchemaVersion(data)
	if err != nil {
		return fmt.Errorf("reading marker %s: %w", path, err)
	}
	if v > MarkerSchemaVersion {
		return fmt.Errorf("refusing to overwrite marker %s: schema_version %d is newer than supported version %d", path, v, MarkerSchemaVersion)
	}
	return nil
}

// writeMarker writes the current marker into the rendered tree after
// refusing to clobber a marker from a newer schema version.
func writeMarker(dir string, item RenderedSkill, mode Mode, allowExecutable bool) error {
	markerPath := filepath.Join(dir, markerFile)
	if err := checkMarkerWritable(markerPath); err != nil {
		return err
	}
	return os.WriteFile(markerPath, markerBytes(item, mode, allowExecutable), 0o644)
}
