package mcptools

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/skills/discover"
	"github.com/danieljustus/symaira-brain/internal/skills/events"
	"github.com/danieljustus/symaira-brain/internal/skills/install"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
	"github.com/danieljustus/symaira-brain/internal/skills/vcs"
	"github.com/danieljustus/symaira-corekit/exitcodes"
	"github.com/danieljustus/symaira-corekit/mcpserver"
)

// registerVersioningTools registers the per-skill versioning tools:
// skills_discover_sources, skills_history, and skills_restore.
func registerVersioningTools(ctx *serverContext) {
	opts := ctx.opts
	logger := ctx.logger

	ctx.registerTool(&mcpserver.Tool{
		Name:        "skills_discover_sources",
		Description: "Discover unmanaged skill sources in known harness roots or explicit paths.",
		InputSchema: json.RawMessage(`{"type":"object","properties":{"paths":{"type":"array","items":{"type":"string"}},"scope":{"type":"string"}}}`),
		Annotations: &mcpserver.ToolAnnotations{Title: "Discover Skill Sources", ReadOnlyHint: true},
		Handler: func(_ context.Context, in json.RawMessage) (any, error) {
			var args struct {
				Paths []string `json:"paths"`
				Scope string   `json:"scope"`
			}
			if err := json.Unmarshal(in, &args); err != nil {
				return nil, exitcodes.Wrap(err, exitcodes.ExitData, exitcodes.KindValidation, "parse arguments")
			}
			scope := render.ScopeUser
			if args.Scope == string(render.ScopeProject) {
				scope = render.ScopeProject
			}
			candidates, err := discover.DiscoverScanned(discover.Options{
				HomeDir:    opts.HomeDir,
				ProjectDir: opts.ProjectDir,
				Scope:      scope,
				Paths:      args.Paths,
			})
			if err != nil {
				return nil, exitcodes.Wrap(err, exitcodes.ExitData, exitcodes.KindValidation, "discover sources")
			}
			return mcpJSON(map[string]any{"candidates": candidates})
		},
	})
	ctx.registerTool(&mcpserver.Tool{
		Name:        "skills_history",
		Description: "List the versioned commit history of a library skill: revision, timestamp, operation (import/update/restore/unknown) and changed files.",
		InputSchema: json.RawMessage(`{"type":"object","properties":{"name":{"type":"string"},"limit":{"type":"integer"}},"required":["name"]}`),
		Annotations: &mcpserver.ToolAnnotations{Title: "Skill History", ReadOnlyHint: true},
		Handler: func(_ context.Context, in json.RawMessage) (any, error) {
			var args struct {
				Name  string `json:"name"`
				Limit int    `json:"limit"`
			}
			if err := json.Unmarshal(in, &args); err != nil {
				return nil, exitcodes.Wrap(err, exitcodes.ExitData, exitcodes.KindValidation, "parse arguments")
			}
			if args.Name == "" {
				return nil, exitcodes.Wrap(fmt.Errorf("name is required"), exitcodes.ExitData, exitcodes.KindValidation, "history")
			}
			limit := args.Limit
			if limit <= 0 {
				limit = 20
			}
			dir := filepath.Join(opts.LibraryDir, args.Name)
			if !vcs.IsRepo(dir) {
				return nil, exitcodes.Wrap(fmt.Errorf("skill %q is not versioned: no git repository at %s", args.Name, dir), exitcodes.ExitData, exitcodes.KindValidation, "history")
			}
			history, err := vcs.History(dir, limit)
			if err != nil {
				return nil, exitcodes.Wrap(err, exitcodes.ExitSoftware, exitcodes.KindInternal, "read history")
			}
			if history == nil {
				history = []vcs.CommitInfo{}
			}
			return mcpJSON(map[string]any{"name": args.Name, "history": history})
		},
	})
	ctx.registerTool(&mcpserver.Tool{
		Name:        "skills_restore",
		Description: "Roll a library skill's files back to a previous revision by forward commit. Dry-run defaults to true; pass dry_run=false to write. Refuses invalid restored states and never discards uncommitted changes (pass allow_dirty=true to snapshot them first); sync=true re-installs stale targets.",
		InputSchema: json.RawMessage(`{"type":"object","properties":{"name":{"type":"string"},"rev":{"type":"string"},"dry_run":{"type":"boolean"},"allow_dirty":{"type":"boolean"},"sync":{"type":"boolean"}},"required":["name","rev"]}`),
		Annotations: &mcpserver.ToolAnnotations{Title: "Restore Skill", ReadOnlyHint: false, DestructiveHint: true},
		Handler: func(_ context.Context, in json.RawMessage) (any, error) {
			var args struct {
				Name       string `json:"name"`
				Rev        string `json:"rev"`
				DryRun     *bool  `json:"dry_run"`
				AllowDirty bool   `json:"allow_dirty"`
				Sync       bool   `json:"sync"`
			}
			if err := json.Unmarshal(in, &args); err != nil {
				return nil, exitcodes.Wrap(err, exitcodes.ExitData, exitcodes.KindValidation, "parse arguments")
			}
			if args.Name == "" || args.Rev == "" {
				return nil, exitcodes.Wrap(fmt.Errorf("name and rev are required"), exitcodes.ExitData, exitcodes.KindValidation, "restore skill")
			}
			dryRun := true
			if args.DryRun != nil {
				dryRun = *args.DryRun
			}
			dir := filepath.Join(opts.LibraryDir, args.Name)
			if !vcs.IsRepo(dir) {
				return nil, exitcodes.Wrap(fmt.Errorf("skill %q is not versioned: no git repository at %s", args.Name, dir), exitcodes.ExitData, exitcodes.KindValidation, "restore skill")
			}
			if opts.VCSEnabled != nil && !*opts.VCSEnabled {
				return nil, exitcodes.Wrap(fmt.Errorf("per-skill versioning is disabled (vcs.enabled = false); restore cannot write to %s", dir), exitcodes.ExitConfig, exitcodes.KindConfig, "restore skill")
			}
			resolved, err := vcs.Resolve(dir, args.Rev)
			if err != nil {
				return nil, exitcodes.Wrap(fmt.Errorf("revision %q not found: %v", args.Rev, err), exitcodes.ExitData, exitcodes.KindValidation, "restore skill")
			}
			dirty, err := vcs.Dirty(dir)
			if err != nil {
				return nil, exitcodes.Wrap(err, exitcodes.ExitSoftware, exitcodes.KindInternal, "check working tree")
			}
			if dirty && !args.AllowDirty {
				return nil, exitcodes.Wrap(fmt.Errorf("skill %q has uncommitted changes; they are never discarded — pass allow_dirty=true to snapshot them into a pre-restore commit first", args.Name), exitcodes.ExitConflict, exitcodes.KindConflict, "restore skill")
			}
			// Materialize the target revision and validate it before any
			// write. The tree is extracted into <tmp>/<name> so the restored
			// frontmatter name is compared against the library directory name
			// by the normal validation rules.
			tmp, err := os.MkdirTemp("", "symskills-restore-*")
			if err != nil {
				return nil, exitcodes.Wrap(err, exitcodes.ExitSoftware, exitcodes.KindInternal, "restore skill")
			}
			defer os.RemoveAll(tmp)
			restoredTree := filepath.Join(tmp, args.Name)
			if err := vcs.ExtractRev(dir, resolved, restoredTree); err != nil {
				return nil, exitcodes.Wrap(err, exitcodes.ExitSoftware, exitcodes.KindInternal, "extract revision")
			}
			if err := validateRestoredMCP(resolved, restoredTree); err != nil {
				return nil, err
			}
			changed, err := vcs.ChangedFiles(dir, resolved)
			if err != nil {
				return nil, exitcodes.Wrap(err, exitcodes.ExitSoftware, exitcodes.KindInternal, "compare revision")
			}
			result := map[string]any{
				"name":          args.Name,
				"rev":           resolved,
				"dry_run":       dryRun,
				"action":        "planned",
				"changed_files": changed,
				"notes": []string{
					"Restore creates a new forward commit; it never resets or rewrites history.",
					"Run symskills sync to reinstall targets that become stale after the restore.",
				},
			}
			if dirty {
				result["notes"] = append(result["notes"].([]string), "Uncommitted changes would be committed as a pre-restore snapshot.")
			}
			if dryRun {
				return mcpJSON(result)
			}
			if dirty {
				if _, err := vcs.Commit(dir, fmt.Sprintf("restore: snapshot uncommitted changes before restore to %s", resolved)); err != nil {
					return nil, exitcodes.Wrap(err, exitcodes.ExitSoftware, exitcodes.KindInternal, "snapshot uncommitted changes")
				}
			}
			head, err := vcs.Restore(dir, restoredTree, fmt.Sprintf("restore: skill %s to %s", args.Name, resolved))
			if err != nil {
				return nil, exitcodes.Wrap(err, exitcodes.ExitSoftware, exitcodes.KindInternal, "restore skill")
			}
			if logger != nil {
				logger.Record(events.Event{
					Event:   events.EventInstall,
					Skill:   args.Name,
					Target:  "restore",
					Outcome: events.OutcomeOK,
					Actor:   events.ActorMCP,
				})
			}
			result["action"] = "restored"
			result["head"] = head
			if args.Sync {
				statuses, serr := install.Status(install.StatusOptions{
					HomeDir:    opts.HomeDir,
					ProjectDir: opts.ProjectDir,
					Scope:      render.ScopeUser,
					LibraryDir: opts.LibraryDir,
					BaseDir:    opts.BaseDir,
				})
				if serr != nil {
					return nil, exitcodes.Wrap(serr, exitcodes.ExitSoftware, exitcodes.KindInternal, "post-restore status")
				}
				var stale []install.InstallStatus
				for _, s := range statuses {
					if (s.Status == install.StatusStale || s.Status == install.StatusHarnessChanged) && s.Name == args.Name {
						stale = append(stale, s)
					}
				}
				result["synced"] = resyncStaleMCP(opts, stale, logger)
			}
			return mcpJSON(result)
		},
	})
}

// validateRestoredMCP checks that a restored tree is a valid skill.
func validateRestoredMCP(rev string, restoredTree string) error {
	bundle, err := skill.LoadBundle(restoredTree)
	if err != nil {
		return exitcodes.Wrap(fmt.Errorf("refusing restore to %s: restored state is not a valid skill: %v", rev, err), exitcodes.ExitData, exitcodes.KindValidation, "restore skill")
	}
	var errors []string
	for _, issue := range skill.Validate(bundle) {
		if issue.Severity == "error" {
			errors = append(errors, issue.Message)
		}
	}
	if len(errors) > 0 {
		return exitcodes.Wrap(fmt.Errorf("refusing restore to %s: restored state fails validation: %s", rev, strings.Join(errors, "; ")), exitcodes.ExitData, exitcodes.KindValidation, "restore skill")
	}
	return nil
}

// resyncStaleMCP re-installs the given stale installs, mirroring the
// reinstall path of `symskills sync` (the #115 resync surface).
func resyncStaleMCP(opts Options, stale []install.InstallStatus, logger *events.Logger) []map[string]any {
	results := install.Sync(stale, install.SyncOptions{
		LibraryDir:     opts.LibraryDir,
		RenderDir:      opts.RenderDir,
		BaseDir:        opts.BaseDir,
		Scope:          render.ScopeUser,
		ConflictPolicy: install.ConflictAbort,
		HomeDir:        opts.HomeDir,
	})
	out := make([]map[string]any, 0, len(results))
	for _, r := range results {
		row := map[string]any{"target": string(r.Target), "name": r.Name}
		if r.Action == "installed" {
			if logger != nil {
				logger.Record(events.Event{
					Event:   events.EventInstall,
					Skill:   r.Name,
					Target:  string(r.Target),
					Scope:   string(render.ScopeUser),
					Mode:    string(r.Mode),
					Path:    r.Path,
					Outcome: events.OutcomeOK,
					Actor:   events.ActorMCP,
				})
			}
		}
		if r.Action == "failed" {
			if logger != nil {
				logger.Record(events.Event{
					Event:   events.EventInstall,
					Skill:   r.Name,
					Target:  string(r.Target),
					Outcome: events.OutcomeError,
					Error:   r.Error,
					Actor:   events.ActorMCP,
				})
			}
		}
		row["action"] = r.Action
		if r.Error != "" {
			row["error"] = r.Error
		}
		if r.Path != "" {
			row["path"] = r.Path
		}
		out = append(out, row)
	}
	return out
}
