package install

import (
	"fmt"
	"path/filepath"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

// SyncResult is one row of sync/restore output, produced by Sync.
type SyncResult struct {
	Target render.Target `json:"target"`
	Name   string        `json:"name"`
	Path   string        `json:"path"`
	// Action is planned | installed | skipped | failed.
	Action string `json:"action"`
	Mode   Mode   `json:"mode,omitempty"`
	Error  string `json:"error,omitempty"`
}

// SyncOptions parametrizes the shared sync/resync policy.
type SyncOptions struct {
	// LibraryDir is the portable skills directory.
	LibraryDir string
	// RenderDir is where rendered artifacts are written.
	RenderDir string
	// BaseDir is the managed-artifact root for base snapshots.
	BaseDir string
	// Scope is user or project.
	Scope render.Scope
	// ConflictPolicy controls what happens when library and installed
	// copy both changed since the last base snapshot.
	ConflictPolicy ConflictPolicy
	// DryRun reports the plan without writing.
	DryRun bool
	// HomeDir is the user home directory used for lock acquisition.
	HomeDir string
}

// CollectConflicts returns the install statuses that would cause Sync to
// abort under ConflictAbort policy. Callers use this to produce a report
// before any writes happen, matching the CLI's abort-before-write behavior.
func CollectConflicts(stales []InstallStatus) []InstallStatus {
	var conflicted []InstallStatus
	for _, st := range stales {
		if st.Status == StatusConflict {
			conflicted = append(conflicted, st)
		}
	}
	return conflicted
}

// Sync is the single source of truth for both `symskills sync` (CLI) and
// the MCP skills_restore path: harness-changed installs are reported
// as skipped (never silently overwritten), conflicts honor the conflict
// policy, and stale installs are re-rendered and re-installed.
func Sync(stales []InstallStatus, opts SyncOptions) []SyncResult {
	results := make([]SyncResult, 0, len(stales))
	for _, st := range stales {
		// Harness-side edits are never silently overwritten.
		if st.Status == StatusHarnessChanged {
			results = append(results, SyncResult{
				Target: st.Target,
				Name:   st.Name,
				Path:   st.Path,
				Action: "skipped",
				Error:  "harness changed; use symskills pull",
				Mode:   st.Mode,
			})
			continue
		}
		// Conflicts: non-prefer-source policies skip; prefer-source
		// falls through to the reinstall path below.
		if st.Status == StatusConflict && opts.ConflictPolicy != ConflictPreferSource {
			results = append(results, SyncResult{
				Target: st.Target,
				Name:   st.Name,
				Path:   st.Path,
				Action: "skipped",
				Error:  "conflict; resolve manually",
				Mode:   st.Mode,
			})
			continue
		}
		// In-sync and other statuses: skip silently.
		if st.Status != StatusStale && st.Status != StatusConflict {
			continue
		}
		// Reinstall candidates: stale installs plus conflicts under
		// prefer-source. Entries with errors other than conflicts are
		// skipped with the error message.
		if st.Error != "" && st.Status != StatusConflict {
			results = append(results, SyncResult{
				Target: st.Target,
				Name:   st.Name,
				Path:   st.Path,
				Action: "skipped",
				Error:  st.Error,
				Mode:   st.Mode,
			})
			continue
		}
		if opts.DryRun {
			results = append(results, SyncResult{
				Target: st.Target,
				Name:   st.Name,
				Path:   st.Path,
				Action: "planned",
				Mode:   st.Mode,
			})
			continue
		}
		bundle, err := skill.LoadBundle(filepath.Join(opts.LibraryDir, st.Name))
		if err != nil {
			results = append(results, SyncResult{
				Target: st.Target,
				Name:   st.Name,
				Path:   st.Path,
				Action: "failed",
				Error:  err.Error(),
			})
			continue
		}
		installOpts := Options{
			Scope:           opts.Scope,
			Mode:            st.Mode,
			BaseDir:         opts.BaseDir,
			AllowExecutable: bundle.Manifest.Skill.AllowExecutable,
		}
		rendered, errs := render.RenderAll(bundle, opts.RenderDir, []render.Target{st.Target})
		if len(rendered) == 0 {
			msg := fmt.Sprintf("target %s produced no render output", st.Target)
			if len(errs) > 0 {
				msg = errs[0].Error()
			}
			results = append(results, SyncResult{
				Target: st.Target,
				Name:   st.Name,
				Path:   st.Path,
				Action: "failed",
				Error:  msg,
			})
			continue
		}
		lock, lockErr := AcquirePullLock(st.Target, rendered[0].Name, PullOptions{HomeDir: opts.HomeDir})
		if lockErr != nil {
			results = append(results, SyncResult{
				Target: st.Target,
				Name:   st.Name,
				Path:   st.Path,
				Action: "skipped",
				Error:  lockErr.Error(),
				Mode:   st.Mode,
			})
			continue
		}
		result, err := Install(RenderedSkill{Target: st.Target, Name: rendered[0].Name, Path: rendered[0].Path}, installOpts)
		_ = lock.Release()
		if err != nil {
			results = append(results, SyncResult{
				Target: st.Target,
				Name:   st.Name,
				Path:   st.Path,
				Action: "failed",
				Error:  err.Error(),
			})
			continue
		}
		results = append(results, SyncResult{
			Target: st.Target,
			Name:   st.Name,
			Path:   result.Path,
			Action: result.Action,
			Mode:   result.Mode,
		})
	}
	return results
}
