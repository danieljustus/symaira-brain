package install

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

const (
	pullManifestFile  = ".symskills-pull.json"
	codexMetadataFile = "agents/openai.yaml"
)

// PullOptions configures a harness-to-library pull. Pull always writes a
// pending tree, never the library itself. ApplyPending is the explicit
// promotion step.
type PullOptions struct {
	HomeDir    string
	ProjectDir string
	Scope      render.Scope
	LibraryDir string
	PendingDir string
	BaseDir    string
	Target     render.Target
	Name       string
	DryRun     bool
}

// PullChange describes one content or resource change that would be staged.
type PullChange struct {
	Path   string `json:"path"`
	Status string `json:"status"` // modified, added, removed
}

// PullFrontmatterChange is kept separate from prose/resource changes because
// frontmatter can alter harness permissions, especially allowed-tools.
type PullFrontmatterChange struct {
	Key    string `json:"key"`
	From   any    `json:"from,omitempty"`
	To     any    `json:"to,omitempty"`
	Reason string `json:"reason,omitempty"`
}

// PullResult is the complete pull plan and staging result.
type PullResult struct {
	Action             string                  `json:"action"`
	Target             render.Target           `json:"target"`
	Name               string                  `json:"name"`
	StagePath          string                  `json:"stage_path,omitempty"`
	Changes            []PullChange            `json:"changes,omitempty"`
	FrontmatterChanges []PullFrontmatterChange `json:"frontmatter_changes,omitempty"`
	Refusals           []string                `json:"refusals,omitempty"`
}

// PendingPath returns the pending tree for target/name.
func PendingPath(target render.Target, name string, opts PullOptions) (string, error) {
	root := opts.PendingDir
	if root == "" {
		home := opts.HomeDir
		if home == "" {
			var err error
			home, err = os.UserHomeDir()
			if err != nil {
				return "", err
			}
		}
		root = filepath.Join(home, ".local", "share", "symskills", "pending")
	}
	if filepath.Base(name) != name || name == "." || name == ".." {
		return "", fmt.Errorf("invalid skill name %q", name)
	}
	return filepath.Join(root, string(target), name), nil
}

// Pull compares the installed target tree with the current library render and
// stages only harness-side changes. Overlay-produced regions are refused.
func Pull(opts PullOptions) (PullResult, error) {
	if opts.Scope == "" {
		opts.Scope = render.ScopeUser
	}
	if opts.Target == "" || opts.Name == "" {
		return PullResult{}, errors.New("pull requires target and skill name")
	}
	if opts.LibraryDir == "" {
		return PullResult{}, errors.New("pull requires library directory")
	}
	lock, err := AcquirePullLock(opts.Target, opts.Name, opts)
	if err != nil {
		return PullResult{Action: "locked", Target: opts.Target, Name: opts.Name}, err
	}
	defer func() { _ = lock.Release() }()
	libraryPath := filepath.Join(opts.LibraryDir, opts.Name)
	bundle, err := skill.LoadBundle(libraryPath)
	if err != nil {
		return PullResult{}, err
	}
	installedPath, err := InstallPath(opts.Target, opts.Name, Options{
		HomeDir: opts.HomeDir, ProjectDir: opts.ProjectDir, Scope: opts.Scope,
	})
	if err != nil {
		return PullResult{}, err
	}
	result := PullResult{Action: "planned", Target: opts.Target, Name: opts.Name, Changes: []PullChange{}, FrontmatterChanges: []PullFrontmatterChange{}, Refusals: []string{}}
	if _, err := os.Stat(installedPath); err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return result, fmt.Errorf("installed skill not found at %s", installedPath)
		}
		return result, err
	}

	// Render into a throwaway tree. Besides giving us the exact fresh output,
	// this avoids touching the live render cache in symlink mode.
	rendered, cleanup, err := render.StagingRender(bundle, []render.Target{opts.Target})
	if err != nil {
		return result, err
	}
	defer cleanup()
	if len(rendered) == 0 {
		return result, fmt.Errorf("target %s produced no render output", opts.Target)
	}
	freshPath := rendered[0].Path

	base, _ := pullBaseHashes(opts)
	left, err := fileHashes(freshPath)
	if err != nil {
		return result, err
	}
	right, err := fileHashes(installedPath)
	if err != nil {
		return result, err
	}
	var driftOutcome DriftOutcome
	if len(base) > 0 {
		drifts := ClassifyDrift(base, left, right)
		driftOutcome = SummarizeDrift(drifts)
		for path := range driftOutcome.Refusable {
			result.Refusals = append(result.Refusals, fmt.Sprintf("conflict in %s", path))
		}
		if len(result.Refusals) > 0 {
			sort.Strings(result.Refusals)
			return result, fmt.Errorf("pull refused: %s", strings.Join(result.Refusals, "; "))
		}
	}

	sourceFM, sourceBody, err := readSkillMarkdown(filepath.Join(libraryPath, "SKILL.md"))
	if err != nil {
		return result, err
	}
	installedFM, installedBody, err := readSkillMarkdown(filepath.Join(installedPath, "SKILL.md"))
	if err != nil {
		return result, err
	}
	if err := pullBody(sourceBody, installedBody, bundle, opts.Target, &result, &sourceBody); err != nil {
		return result, err
	}
	if err := pullFrontmatter(sourceFM, installedFM, bundle, opts.Target, &result); err != nil {
		return result, err
	}

	// Copy resource changes from the harness tree. Marker and generated target
	// metadata are bookkeeping/output, not portable source files.
	for _, path := range unionPaths(left, right, base) {
		if isNeverPulled(path, opts.Target) || path == "SKILL.md" {
			continue
		}
		ld, lok := left[path]
		rd, rok := right[path]
		bok := false
		if len(base) > 0 {
			_, bok = base[path]
		}
		if len(base) > 0 && !driftOutcome.Pullable[path] {
			continue
		}
		if !rok {
			if bok && lok {
				result.Changes = append(result.Changes, PullChange{Path: path, Status: "removed"})
			}
			continue
		}
		if !lok || ld != rd {
			status := "modified"
			if !lok {
				status = "added"
			}
			result.Changes = append(result.Changes, PullChange{Path: path, Status: status})
		}
	}
	sort.Slice(result.Changes, func(i, j int) bool { return result.Changes[i].Path < result.Changes[j].Path })
	if len(result.Changes) == 0 && sourceBody == bodyFromBundle(bundle) && len(result.FrontmatterChanges) == 0 {
		result.Action = "current"
	}
	if opts.DryRun {
		result.Action = "planned"
		return result, nil
	}
	stage, err := PendingPath(opts.Target, opts.Name, opts)
	if err != nil {
		return result, err
	}
	if err := stagePullTree(stage, libraryPath, installedPath, sourceFM, sourceBody, result.Changes, opts.Target); err != nil {
		return result, err
	}
	result.StagePath = stage
	result.Action = "staged"
	return result, nil
}

func pullBaseHashes(opts PullOptions) (map[string]string, error) {
	baseDir, err := basePathForRead(opts.Target, opts.Name, Options{HomeDir: opts.HomeDir, ProjectDir: opts.ProjectDir, Scope: opts.Scope, BaseDir: opts.BaseDir})
	if err != nil {
		return nil, err
	}
	return baseHashes(baseDir)
}
