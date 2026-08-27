package install

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

// StatusKind classifies one entry in a harness skill root relative to the
// library source.
type StatusKind string

const (
	// StatusInSync reports that the installed bundle matches the current
	// library source.
	StatusInSync StatusKind = "in-sync"
	// StatusStale reports that the library source changed since the last
	// install; `symskills sync` repairs it.
	StatusStale StatusKind = "stale"
	// StatusHarnessChanged reports the installed copy diverged from the
	// base snapshot while the library did not. The edit lives on the
	// harness side and would be lost by a reinstall; `symskills pull`
	// carries it back into the library. It is never reported as stale.
	StatusHarnessChanged StatusKind = "harness-changed"
	// StatusConflict reports the library and the installed copy both
	// diverged from the base to different content. No automatic repair
	// exists; with the default policy `symskills sync` aborts with a
	// report naming target, skill and file, and writes nothing.
	StatusConflict StatusKind = "conflict"
	// StatusOrphaned reports a symskills-managed install whose library
	// source directory no longer exists. Orphaned installs are reported
	// and never auto-removed.
	StatusOrphaned StatusKind = "orphaned"
	// StatusUnmanaged reports a directory in a harness skill root that
	// symskills never installed (no marker). Unmanaged skills are
	// reported and never written to.
	StatusUnmanaged StatusKind = "unmanaged"
)

// InstallStatus is one row of `symskills status`: a single entry in a
// harness skill root with its drift classification.
type InstallStatus struct {
	Target      render.Target `json:"target"`
	Name        string        `json:"name"`
	Path        string        `json:"path"`
	Status      StatusKind    `json:"status"`
	Mode        Mode          `json:"mode,omitempty"`
	InstalledAt string        `json:"installed_at,omitempty"`
	SourceHash  string        `json:"source_hash,omitempty"`
	// Error carries the reason an install could not be classified or
	// re-installed (broken bundle, failed comparison render). The status
	// stays one of the kinds above; sync skips entries with an error.
	Error string `json:"error,omitempty"`
	// Drift carries the per-file three-way classification for installs
	// that diverge from the base snapshot (harness-changed and conflict).
	// It names the files a pull would carry back or a conflict report
	// must mention.
	Drift []FileDrift `json:"drift,omitempty"`
}

// StatusOptions configures a fleet-wide status scan.
type StatusOptions struct {
	HomeDir    string
	ProjectDir string
	Scope      render.Scope
	LibraryDir string
	BaseDir    string
	CacheDir   string
	// Targets limits the scan; empty means every registered target.
	Targets []render.Target
	// Skills limits the scan to these install names; empty means all.
	Skills []string
}

// Status reports every entry in every target's skill root, classified as
// in-sync, stale, harness-changed, conflict, orphaned or unmanaged relative
// to the library source.
//
// The comparison signal is Marker.SourceHash — the render-cache key install
// writes — against a freshly computed hash of the exact same form. The fresh
// hash is obtained by rendering the current library bundle into a throwaway
// staging directory and reading the marker the render writes there, so the
// computation is reused verbatim (render.StagingRender + writeRendered),
// never reimplemented. The scan writes nothing outside the staging
// directories, which are removed before Status returns.
//
// When a base snapshot (#124) exists for an install, the coarse hash
// comparison is replaced by a per-file three-way classification (base /
// fresh render / installed, #126) that separates library drift (stale, push
// direction) from harness-side edits (harness-changed, pull direction) and
// reports both-sides divergence as conflict. The hash comparison remains
// the fallback for installs that predate base snapshots.
func Status(opts StatusOptions) ([]InstallStatus, error) {
	if opts.HomeDir == "" {
		if h, err := os.UserHomeDir(); err == nil {
			opts.HomeDir = h
		}
	}
	if opts.Scope == "" {
		opts.Scope = render.ScopeUser
	}
	targets := opts.Targets
	if len(targets) == 0 {
		targets = render.DefaultTargets()
	}

	// pending is one managed install waiting for its library comparison.
	type pending struct {
		target render.Target
		path   string
		marker Marker
	}
	installs := map[string][]pending{}
	var out []InstallStatus
	for _, target := range targets {
		root, err := TargetDir(target, Options{HomeDir: opts.HomeDir, ProjectDir: opts.ProjectDir, Scope: opts.Scope})
		if err != nil {
			return nil, err
		}
		entries, err := os.ReadDir(root)
		if errors.Is(err, os.ErrNotExist) {
			continue
		}
		if err != nil {
			return nil, fmt.Errorf("scan skill root %s: %w", root, err)
		}
		for _, entry := range entries {
			if !entry.IsDir() && entry.Type()&os.ModeSymlink == 0 {
				continue
			}
			path := filepath.Join(root, entry.Name())
			marker, managed, merr := readInstallMarker(path)
			if !managed {
				out = append(out, InstallStatus{Target: target, Name: entry.Name(), Path: path, Status: StatusUnmanaged})
				continue
			}
			st := InstallStatus{
				Target:      target,
				Name:        entry.Name(),
				Path:        path,
				Status:      StatusStale,
				Mode:        marker.Mode,
				InstalledAt: marker.Installed,
				SourceHash:  marker.SourceHash,
			}
			if merr != nil {
				st.Error = merr.Error()
				out = append(out, st)
				continue
			}
			installs[entry.Name()] = append(installs[entry.Name()], pending{target: target, path: path, marker: marker})
		}
	}

	names := make([]string, 0, len(installs))
	for name := range installs {
		names = append(names, name)
	}
	sort.Strings(names)

	var cleanups []func()
	defer func() {
		for _, c := range cleanups {
			c()
		}
	}()
	for _, name := range names {
		pend := installs[name]
		src := filepath.Join(opts.LibraryDir, name)
		if !isSkillSource(src) {
			for _, p := range pend {
				out = append(out, InstallStatus{
					Target: p.target, Name: name, Path: p.path, Status: StatusOrphaned,
					Mode: p.marker.Mode, InstalledAt: p.marker.Installed, SourceHash: p.marker.SourceHash,
				})
			}
			continue
		}
		bundle, err := skill.LoadBundle(src)
		if err != nil {
			for _, p := range pend {
				out = append(out, InstallStatus{
					Target: p.target, Name: name, Path: p.path, Status: StatusStale,
					Mode: p.marker.Mode, InstalledAt: p.marker.Installed, SourceHash: p.marker.SourceHash,
					Error: err.Error(),
				})
			}
			continue
		}
		need := make([]render.Target, 0, len(pend))
		seen := map[render.Target]bool{}
		for _, p := range pend {
			if !seen[p.target] {
				seen[p.target] = true
				need = append(need, p.target)
			}
		}
		rendered, cleanup, err := render.CachedStagingRender(bundle, need, opts.CacheDir)
		if err != nil {
			for _, p := range pend {
				out = append(out, InstallStatus{
					Target: p.target, Name: name, Path: p.path, Status: StatusStale,
					Mode: p.marker.Mode, InstalledAt: p.marker.Installed, SourceHash: p.marker.SourceHash,
					Error: err.Error(),
				})
			}
			continue
		}
		cleanups = append(cleanups, cleanup)
		// fresh holds the staging marker (source hash and rendered path)
		// per target, produced by the exact render pipeline install uses.
		fresh := map[render.Target]struct {
			hash string
			path string
		}{}
		for _, item := range rendered {
			m, ok, merr := readInstallMarker(item.Path)
			if ok && merr == nil {
				fresh[item.Target] = struct {
					hash string
					path string
				}{hash: m.SourceHash, path: item.Path}
			}
		}
		for _, p := range pend {
			st := InstallStatus{
				Target: p.target, Name: name, Path: p.path, Status: StatusStale,
				Mode: p.marker.Mode, InstalledAt: p.marker.Installed, SourceHash: p.marker.SourceHash,
			}
			f, ok := fresh[p.target]
			if !ok {
				st.Error = fmt.Sprintf("comparison render produced no output for target %s", p.target)
				out = append(out, st)
				continue
			}
			// The three-way per-file classification over base / fresh render /
			// installed digests is the authoritative signal whenever a base
			// snapshot exists (#126): the coarse source-hash comparison cannot
			// tell a harness-side edit from library drift and would report
			// either as stale. Harness edits are reported as harness-changed,
			// never as stale, so sync refuses to overwrite them.
			if classified := classifyStatusInstall(p.target, name, p.path, f.path, p.marker, opts); classified != nil {
				out = append(out, *classified)
				continue
			}
			//nolint:staticcheck // f.hash is a variable, not a constant, so a tagged switch is not possible here
			switch {
			case p.marker.SourceHash == f.hash:
				st.Status = StatusInSync
			case p.marker.SourceHash == "":
				// The install predates source hashes: fall back to a
				// content comparison so a matching copy is not reported
				// stale forever.
				changes, derr := Diff(f.path, p.path, Options{Scope: opts.Scope, Target: p.target, BaseDir: opts.BaseDir})
				if derr == nil && len(changes) == 0 {
					st.Status = StatusInSync
				}
			}
			out = append(out, st)
		}
	}

	if len(opts.Skills) > 0 {
		want := make(map[string]bool, len(opts.Skills))
		for _, s := range opts.Skills {
			want[s] = true
		}
		filtered := out[:0]
		for _, st := range out {
			if want[st.Name] {
				filtered = append(filtered, st)
			}
		}
		out = filtered
	}

	sort.Slice(out, func(i, j int) bool {
		if out[i].Target != out[j].Target {
			return out[i].Target < out[j].Target
		}
		return out[i].Name < out[j].Name
	})
	return out, nil
}

// classifyStatusInstall runs the three-way per-file classification for one
// managed install against its base snapshot. It returns nil when no base
// snapshot exists (or it cannot be read), so callers fall back to the
// coarse source-hash comparison — the pre-#124 behavior for installs that
// predate base snapshots.
func classifyStatusInstall(target render.Target, name, installedPath, freshPath string, marker Marker, opts StatusOptions) *InstallStatus {
	baseDir, err := BasePath(target, name, Options{HomeDir: opts.HomeDir, ProjectDir: opts.ProjectDir, Scope: opts.Scope, BaseDir: opts.BaseDir})
	if err != nil {
		return nil
	}
	base, err := baseHashes(baseDir)
	if err != nil {
		return nil
	}
	left, err := fileHashes(freshPath)
	if err != nil {
		return nil
	}
	right, err := fileHashes(installedPath)
	if err != nil {
		return nil
	}
	drifts := ClassifyDrift(base, left, right)
	outcome := SummarizeDrift(drifts)
	st := &InstallStatus{
		Target: target, Name: name, Path: installedPath, Status: outcome.Status,
		Mode: marker.Mode, InstalledAt: marker.Installed, SourceHash: marker.SourceHash,
	}
	if outcome.Status == StatusConflict || outcome.Status == StatusHarnessChanged {
		st.Drift = drifts
	}
	if outcome.Status == StatusConflict {
		conflictFiles := make([]string, 0, len(outcome.Refusable))
		for path := range outcome.Refusable {
			conflictFiles = append(conflictFiles, path)
		}
		sort.Strings(conflictFiles)
		st.Error = fmt.Sprintf("conflict in: %s", strings.Join(conflictFiles, ", "))
	}
	return st
}

// readInstallMarker resolves path (following a symlink, as symlink-mode
// installs point into the render cache) and parses the .symskills.json
// marker inside. managed is false when the path carries no marker file,
// meaning symskills does not manage it. A present but unreadable marker is
// still managed, with the parse error returned for diagnosis.
func readInstallMarker(path string) (m Marker, managed bool, err error) {
	resolved := path
	if fi, lerr := os.Lstat(path); lerr == nil && fi.Mode()&os.ModeSymlink != 0 {
		target, rerr := os.Readlink(path)
		if rerr != nil {
			return m, false, nil
		}
		if !filepath.IsAbs(target) {
			target = filepath.Join(filepath.Dir(path), target)
		}
		resolved = target
	}
	data, rerr := os.ReadFile(filepath.Join(resolved, markerFile))
	if rerr != nil {
		return m, false, nil
	}
	if err := json.Unmarshal(data, &m); err != nil {
		return m, true, fmt.Errorf("reading marker %s: %w", path, err)
	}
	return m, true, nil
}

// isSkillSource reports whether dir contains a SKILL.md, i.e. a library
// source directory still exists. It distinguishes "source deleted"
// (orphaned) from "source present but broken" (stale with a diagnostic).
func isSkillSource(dir string) bool {
	_, err := os.Stat(filepath.Join(dir, "SKILL.md"))
	return err == nil
}
