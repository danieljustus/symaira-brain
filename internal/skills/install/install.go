// Package install installs rendered skill folders into supported harness paths.
package install

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/skills/fsutil"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

const markerFile = ".symskills.json"

// MarkerSchemaVersion is the .symskills.json marker format version this
// build understands and writes. docs/marker-protocol.md is the contract the
// macOS client is bound by; bump only on incompatible marker changes.
const MarkerSchemaVersion = 1

type Mode string

const (
	ModeSymlink Mode = "symlink"
	ModeCopy    Mode = "copy"
)

type RenderedSkill struct {
	Target render.Target `json:"target"`
	Name   string        `json:"name"`
	Path   string        `json:"path"`
}

type Options struct {
	HomeDir    string       `json:"home_dir"`
	ProjectDir string       `json:"project_dir"`
	Scope      render.Scope `json:"scope"`
	Mode       Mode         `json:"mode"`
	DryRun     bool         `json:"dry_run"`
	// Force adopts a destination that was not installed by symskills. The
	// existing directory is moved to a backup location instead of being
	// deleted, so a hand-written skill is never lost silently.
	Force bool `json:"force"`
	// AllowExecutable preserves executable bits on resource files instead
	// of stripping them (the default install policy).
	AllowExecutable bool `json:"allow_executable"`
	// BaseDir overrides the base snapshot root (defaults to
	// ~/.local/share/symskills/base). It is written on install and removed
	// on uninstall.
	BaseDir string `json:"base_dir,omitempty"`
	// Target identifies the harness a skill is installed into. It is used
	// by Diff to locate the persisted base snapshot.
	Target render.Target `json:"target,omitempty"`
}

// ResourceModeChange describes an executable-bit change applied (or planned)
// to one resource file during install. Modes are octal strings like "0755".
type ResourceModeChange struct {
	Path string `json:"path"`
	From string `json:"from"`
	To   string `json:"to"`
}

type Result struct {
	Action string        `json:"action"`
	Target render.Target `json:"target"`
	Name   string        `json:"name"`
	Path   string        `json:"path"`
	Mode   Mode          `json:"mode"`
	// BackupPath is set when --force adopted an unmanaged destination and
	// moved it aside.
	BackupPath string `json:"backup_path,omitempty"`
	// ModeChanges lists every resource whose executable bit was stripped
	// (or would be stripped in a dry run). Empty when AllowExecutable is
	// set or the bundle has no executable resources.
	ModeChanges []ResourceModeChange `json:"mode_changes,omitempty"`
}

type Marker struct {
	SchemaVersion int           `json:"schema_version,omitempty"`
	ManagedBy     string        `json:"managed_by"`
	Target        render.Target `json:"target"`
	Name          string        `json:"name"`
	RenderedAt    string        `json:"rendered_at"`
	Mode          Mode          `json:"mode"`
	Installed     string        `json:"installed"`
	SourceHash    string        `json:"source_hash,omitempty"`
	// AllowExecutable records whether the install preserved executable
	// bits (--allow-executable or the manifest setting). Additive field;
	// sync replays it so a re-install never silently strips bits.
	AllowExecutable bool `json:"allow_executable,omitempty"`
}

type Change struct {
	Path   string `json:"path"`
	Status string `json:"status"`
	// Diff is a unified-style content diff (left = freshly rendered,
	// right = installed copy) for modified files. It is best-effort
	// enrichment: empty for added/removed files and for modified files
	// whose content cannot be diffed (binary, unreadable, or too large).
	// Additive field — consumers that read only path/status are unaffected.
	Diff string `json:"diff,omitempty"`
}

func Install(item RenderedSkill, opts Options) (Result, error) {
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
		if _, err := os.Stat(filepath.Join(target, markerFile)); err != nil {
			return fmt.Errorf("refusing to overwrite unmanaged skill at %s", path)
		}
		return checkMarkerWritable(filepath.Join(target, markerFile))
	}
	if _, err := os.Stat(filepath.Join(path, markerFile)); err != nil {
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

func InstallPath(target render.Target, name string, opts Options) (string, error) {
	if err := skill.ValidateSkillName(name); err != nil {
		return "", fmt.Errorf("invalid install name for target %s: %w", target, err)
	}
	home := opts.HomeDir
	if home == "" {
		var err error
		home, err = os.UserHomeDir()
		if err != nil {
			return "", err
		}
	}
	project := opts.ProjectDir
	if project == "" {
		cwd, err := os.Getwd()
		if err == nil {
			project = cwd
		}
	}
	if opts.Scope == "" {
		opts.Scope = render.ScopeUser
	}
	spec, ok := render.LookupSpec(target)
	if !ok {
		return "", fmt.Errorf("unknown target %s", target)
	}
	root := spec.SkillRoot(home, project, opts.Scope)
	return filepath.Join(root, name), nil
}

// TargetDir returns the base installation directory for a target without requiring a skill name.
func TargetDir(target render.Target, opts Options) (string, error) {
	path, err := InstallPath(target, "placeholder", opts)
	if err != nil {
		return "", err
	}
	return filepath.Dir(path), nil
}

// Uninstall removes a managed installed skill. It reports whether an
// installation was actually removed (removed == false means nothing was
// installed at the resolved path).
func Uninstall(target render.Target, name string, opts Options) (removed bool, err error) {
	dest, err := InstallPath(target, name, opts)
	if err != nil {
		return false, err
	}
	st, err := os.Lstat(dest)
	if errors.Is(err, os.ErrNotExist) {
		return false, nil
	} else if err != nil {
		return false, err
	}
	if st.Mode()&os.ModeSymlink != 0 {
		linkTarget, err := os.Readlink(dest)
		if err != nil {
			return false, err
		}
		if !filepath.IsAbs(linkTarget) {
			linkTarget = filepath.Join(filepath.Dir(dest), linkTarget)
		}
		if _, err := os.Stat(linkTarget); !errors.Is(err, os.ErrNotExist) {
			if _, err := os.Stat(filepath.Join(linkTarget, markerFile)); err != nil {
				return false, fmt.Errorf("refusing to remove unmanaged skill at %s", dest)
			}
		}
		if err := os.RemoveAll(dest); err != nil {
			return false, err
		}
		if err := RemoveBase(target, name, opts); err != nil {
			return false, err
		}
		return true, nil
	}
	if _, err := os.Stat(filepath.Join(dest, markerFile)); err != nil {
		return false, fmt.Errorf("refusing to remove unmanaged skill at %s", dest)
	}
	if err := os.RemoveAll(dest); err != nil {
		return false, err
	}
	if err := RemoveBase(target, name, opts); err != nil {
		return false, err
	}
	return true, nil
}

// Diff compares a freshly rendered skill against its installed state.
func Diff(renderedPath, installedPath string, opts Options) ([]Change, error) {
	// #123: when the install target is a symlink into the render directory
	// (the default symlink mode), a two-way comparison is structurally
	// blind — both paths resolve to the same tree, so every digest matches
	// itself and drift is never reported. When a base snapshot (#124)
	// exists for this target/name, anchor the comparison on it instead:
	// harness-side edits appear in installed-vs-base and library drift in
	// rendered-vs-base. Without a base (never managed, or installed before
	// base snapshots existed) the legacy comparison runs.
	if opts.BaseDir != "" && opts.Target != "" && isSymlink(installedPath) {
		baseDir, err := BasePath(opts.Target, filepath.Base(renderedPath), opts)
		if err == nil {
			if _, statErr := os.Stat(filepath.Join(baseDir, manifestFile)); statErr == nil {
				return diffAgainstBase(renderedPath, installedPath, baseDir)
			}
		}
	}
	return diffTwoWay(renderedPath, installedPath)
}

// diffTwoWay is the legacy two-path comparison: the change list answers
// "what would change if the installed tree were replaced by the rendered
// tree" (added = only in rendered, modified = different content, removed =
// only in installed). The install marker is excluded from both sides.
func diffTwoWay(renderedPath, installedPath string) ([]Change, error) {
	left, err := fileHashes(renderedPath)
	if err != nil {
		return nil, err
	}
	right, err := fileHashes(installedPath)
	if err != nil {
		return nil, err
	}
	seen := map[string]bool{}
	changes := []Change{}
	for path, lhash := range left {
		seen[path] = true
		if rhash, ok := right[path]; !ok {
			changes = append(changes, Change{Path: path, Status: "added"})
		} else if rhash != lhash {
			changes = append(changes, Change{Path: path, Status: "modified", Diff: diffFileContent(filepath.ToSlash(path), filepath.Join(renderedPath, path), filepath.Join(installedPath, path))})
		}
	}
	for path := range right {
		if !seen[path] {
			changes = append(changes, Change{Path: path, Status: "removed"})
		}
	}
	sort.Slice(changes, func(i, j int) bool { return changes[i].Path < changes[j].Path })
	return changes, nil
}

// diffAgainstBase compares both the rendered tree and the installed tree
// against the persisted base snapshot. A file is modified when either side
// diverges from the base; added when it exists on a side but not in the
// base; removed when it exists only in the base. The snapshot's own
// manifest.json is excluded from the base side.
func diffAgainstBase(renderedPath, installedPath, baseDir string) ([]Change, error) {
	rendered, err := fileHashes(renderedPath)
	if err != nil {
		return nil, err
	}
	installed, err := fileHashes(installedPath)
	if err != nil {
		return nil, err
	}
	base, err := fileHashes(baseDir)
	if err != nil {
		return nil, err
	}
	delete(base, manifestFile)

	status := map[string]string{}
	paths := map[string]bool{}
	for path := range rendered {
		paths[path] = true
	}
	for path := range installed {
		paths[path] = true
	}
	for path := range base {
		paths[path] = true
	}
	for path := range paths {
		_, inRendered := rendered[path]
		_, inInstalled := installed[path]
		_, inBase := base[path]
		switch {
		case inBase && !inRendered && !inInstalled:
			status[path] = "removed"
		case !inBase && !inRendered && inInstalled:
			status[path] = "added" // harness-side addition
		case !inBase && inRendered:
			status[path] = "added"
		case inBase && inRendered && !inInstalled:
			status[path] = "removed" // deleted from the harness side
		case inBase && !inRendered && inInstalled:
			status[path] = "removed" // kept only on the harness side
		default:
			if installed[path] != base[path] || rendered[path] != base[path] {
				status[path] = "modified"
			}
		}
	}
	changes := []Change{}
	for path, st := range status {
		c := Change{Path: path, Status: st}
		if st == "modified" {
			// Left = freshly rendered staging copy, right = installed
			// copy. In symlink mode the two trees differ (staging render
			// vs render cache), so this shows the real drift the dialog
			// is asking about — harness-side edits and library changes
			// combined, exactly like the two-way comparison.
			c.Diff = diffFileContent(filepath.ToSlash(path), filepath.Join(renderedPath, path), filepath.Join(installedPath, path))
		}
		changes = append(changes, c)
	}
	sort.Slice(changes, func(i, j int) bool { return changes[i].Path < changes[j].Path })
	return changes, nil
}

// diffLine is one line of an edit script: unchanged (' '), present only in
// the left/rendered side ('-') or only in the right/installed side ('+').
type diffLine struct {
	kind byte
	text string
}

const (
	// maxDiffCells bounds the LCS dynamic-programming table (product of the
	// two line counts) so pathological inputs cannot exhaust memory; files
	// beyond the bound are reported status-only.
	maxDiffCells = 1 << 22
	// maxDiffFileSize bounds how much of a file is read for a content diff;
	// larger files (multi-MiB resources) are reported status-only.
	maxDiffFileSize = 1 << 20
	// diffContext is the number of unchanged context lines around each
	// change in the unified output.
	diffContext = 3
)

// diffFileContent computes a unified-style content diff between leftPath
// (freshly rendered) and rightPath (installed copy). displayPath names the
// file in the diff headers. It is best-effort enrichment: an empty result
// means "no content diff available" (either side missing or binary, file
// too large, or an unreadable file) — the change list itself is never
// downgraded by a diff failure.
func diffFileContent(displayPath, leftPath, rightPath string) string {
	left, err := readDiffLines(leftPath)
	if err != nil {
		return ""
	}
	right, err := readDiffLines(rightPath)
	if err != nil {
		return ""
	}
	if left == nil || right == nil {
		return ""
	}
	if len(left)*len(right) > maxDiffCells {
		return ""
	}
	return unifiedLineDiff(displayPath, left, right)
}

// readDiffLines returns the file's content as lines (trailing newline
// stripped). It returns (nil, nil) for a missing or binary file — callers
// treat that as "no content diff". A present-but-empty file yields an empty
// (non-nil) slice so it can be diffed against a non-empty counterpart.
func readDiffLines(path string) ([]string, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil, nil
		}
		return nil, err
	}
	if len(data) > maxDiffFileSize || bytes.IndexByte(data, 0) >= 0 {
		return nil, nil
	}
	if len(data) == 0 {
		return []string{}, nil
	}
	// Strip one trailing newline so the split does not produce a phantom
	// empty final line.
	return strings.Split(strings.TrimSuffix(string(data), "\n"), "\n"), nil
}

// unifiedLineDiff renders left vs right as a unified diff with ---/+++
// file headers and @@ hunk headers, using an LCS-based edit script.
// displayPath names the file in the headers.
func unifiedLineDiff(displayPath string, left, right []string) string {
	return formatUnifiedDiff(displayPath, editScript(left, right))
}

// editScript computes an LCS-based edit script transforming left into
// right: ' ' unchanged, '-' only in left, '+' only in right, in
// left-to-right order.
func editScript(left, right []string) []diffLine {
	n, m := len(left), len(right)
	// lcs[i][j] = LCS length of left[:i] vs right[:j], packed row-major.
	table := make([]int32, (n+1)*(m+1))
	at := func(i, j int) int { return i*(m+1) + j }
	for i := 1; i <= n; i++ {
		for j := 1; j <= m; j++ {
			switch {
			case left[i-1] == right[j-1]:
				table[at(i, j)] = table[at(i-1, j-1)] + 1
			case table[at(i-1, j)] >= table[at(i, j-1)]:
				table[at(i, j)] = table[at(i-1, j)]
			default:
				table[at(i, j)] = table[at(i, j-1)]
			}
		}
	}
	script := make([]diffLine, 0, n+m)
	i, j := n, m
	for i > 0 && j > 0 {
		switch {
		case left[i-1] == right[j-1]:
			script = append(script, diffLine{kind: ' ', text: left[i-1]})
			i--
			j--
		case table[at(i-1, j)] >= table[at(i, j-1)]:
			script = append(script, diffLine{kind: '-', text: left[i-1]})
			i--
		default:
			script = append(script, diffLine{kind: '+', text: right[j-1]})
			j--
		}
	}
	for ; i > 0; i-- {
		script = append(script, diffLine{kind: '-', text: left[i-1]})
	}
	for ; j > 0; j-- {
		script = append(script, diffLine{kind: '+', text: right[j-1]})
	}
	for a, b := 0, len(script)-1; a < b; a, b = a+1, b-1 {
		script[a], script[b] = script[b], script[a]
	}
	return script
}

// formatUnifiedDiff renders an edit script in unified-diff form: per-file
// ---/+++ headers, @@ hunk headers with old/new line ranges, and lines
// prefixed with ' ', '-', or '+'. Hunks are merged when their changes come
// within diffContext lines of each other.
func formatUnifiedDiff(displayPath string, script []diffLine) string {
	var b strings.Builder
	fmt.Fprintf(&b, "--- a/%s\n+++ b/%s\n", displayPath, displayPath)
	changed := make([]int, 0, len(script))
	for i := range script {
		if script[i].kind != ' ' {
			changed = append(changed, i)
		}
	}
	for i := 0; i < len(changed); {
		start := changed[i] - diffContext
		if start < 0 {
			start = 0
		}
		end := changed[i] + diffContext + 1
		if end > len(script) {
			end = len(script)
		}
		i++
		for i < len(changed) && changed[i] < end {
			end = changed[i] + diffContext + 1
			if end > len(script) {
				end = len(script)
			}
			i++
		}
		var oldStart, newStart, oldCount, newCount int
		for p := start; p < end; p++ {
			switch script[p].kind {
			case ' ':
				if oldStart == 0 {
					oldStart = p + 1
				}
				oldCount++
				if newStart == 0 {
					newStart = p + 1
				}
				newCount++
			case '-':
				if oldStart == 0 {
					oldStart = p + 1
				}
				oldCount++
			case '+':
				if newStart == 0 {
					newStart = p + 1
				}
				newCount++
			}
		}
		fmt.Fprintf(&b, "@@ -%d,%d +%d,%d @@\n", oldStart, oldCount, newStart, newCount)
		for p := start; p < end; p++ {
			b.WriteByte(script[p].kind)
			b.WriteString(script[p].text)
			b.WriteByte('\n')
		}
	}
	return b.String()
}

// isSymlink reports whether path is a symbolic link (install symlink mode).
func isSymlink(path string) bool {
	fi, err := os.Lstat(path)
	return err == nil && fi.Mode()&os.ModeSymlink != 0
}

func fileHashes(root string) (map[string]string, error) {
	out := map[string]string{}
	if _, err := os.Stat(root); errors.Is(err, os.ErrNotExist) {
		return out, nil
	}
	// Resolve any symlink in the root path before walking, so WalkDir
	// sees the actual directory and Lstat-based DirEntry.IsDir() works
	// for symlink-mode installs where root is a symlink to a directory.
	if resolved, err := filepath.EvalSymlinks(root); err == nil {
		root = resolved
	}
	err := filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.Type()&os.ModeSymlink != 0 {
			return nil
		}
		if d.IsDir() {
			return nil
		}
		rel, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		if rel == markerFile {
			return nil
		}
		f, err := os.Open(path)
		if err != nil {
			return err
		}
		hasher := sha256.New()
		if _, err := io.Copy(hasher, f); err != nil {
			_ = f.Close()
			return err
		}
		if err := f.Close(); err != nil {
			return err
		}
		out[rel] = hex.EncodeToString(hasher.Sum(nil))
		return nil
	})
	return out, err
}

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
