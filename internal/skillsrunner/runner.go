package skillsrunner

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-brain/internal/skills/config"
	"github.com/danieljustus/symaira-brain/internal/skills/install"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

// HarnessMap maps registered harness names to in-process skill targets.
// It is derived once from the single harness registry; empty SkillTarget
// values remain valid harnesses on the runner's skipped path.
var HarnessMap = buildHarnessMap()

func buildHarnessMap() map[string]render.Target {
	targets := make(map[string]render.Target)
	for _, h := range harness.All {
		if h.SkillTarget != harness.SkillTargetNone {
			targets[string(h.Name)] = render.Target(h.SkillTarget)
		}
	}
	return targets
}

// DefaultTimeout is the per-target budget for render+install work. It
// mirrors the 30s best effort the legacy bridge applied per symskills
// invocation; a target that exceeds it is reported as an error, not
// silently dropped.
const DefaultTimeout = 30 * time.Second

// Result is the per-target outcome contract preserved from the legacy
// bridge: one entry per requested harness name, named Target.
type Result struct {
	Target  string `json:"target"`
	Status  string `json:"status"`
	Message string `json:"message,omitempty"`
}

// Options plants the skills directories. Empty fields fall back to the
// symskills default paths under $HOME (config.Defaults), the same XDG
// locations the old binary used; tests override Home/Library/Render
// to keep everything inside a temp dir.
type Options struct {
	LibraryDir string
	RenderDir  string
	BaseDir    string
	HomeDir    string
	ProjectDir string
	Timeout    time.Duration
}

// DefaultOptions returns the skills directories a released symbrain
// uses: ~/.local/share/symskills/{library,rendered,base}. Callers that
// need a hermetic run (tests) override fields explicitly.
func DefaultOptions() Options {
	cfg := config.Defaults()
	return Options{
		LibraryDir: cfg.LibraryDir,
		RenderDir:  cfg.RenderDir,
		BaseDir:    cfg.BaseDir,
	}
}

// Run processes every requested harness through the in-process skills
// pipeline, returning one Result per harness. No subprocess is started
// and PATH is never consulted: a symskills binary, present or absent,
// cannot change the plan. dryRun renders in memory and resolves install
// destinations without writing anywhere.
func Run(ctx context.Context, harnessNames []string, opts Options, dryRun bool) ([]Result, error) {
	opts, err := opts.withDefaults()
	if err != nil {
		return nil, fmt.Errorf("skillsrunner: %w", err)
	}

	results := make([]Result, 0, len(harnessNames))
	for _, harness := range harnessNames {
		target, ok := HarnessMap[harness]
		if !ok {
			results = append(results, Result{
				Target:  harness,
				Status:  "skipped",
				Message: fmt.Sprintf("no skill target for harness %q", harness),
			})
			continue
		}

		targetCtx, cancel := context.WithTimeout(ctx, opts.Timeout)
		results = append(results, syncTarget(targetCtx, target, opts, dryRun))
		cancel()
	}
	return results, nil
}

// withDefaults fills empty option fields from the symskills default paths.
func (o Options) withDefaults() (Options, error) {
	cfg := config.Defaults()
	if o.LibraryDir == "" {
		o.LibraryDir = cfg.LibraryDir
	}
	if o.RenderDir == "" {
		o.RenderDir = cfg.RenderDir
	}
	if o.BaseDir == "" {
		o.BaseDir = cfg.BaseDir
	}
	if o.HomeDir == "" {
		home, err := os.UserHomeDir()
		if err != nil {
			return o, fmt.Errorf("resolve home directory: %w", err)
		}
		o.HomeDir = home
	}
	if o.Timeout == 0 {
		o.Timeout = DefaultTimeout
	}
	return o, nil
}

// syncTarget renders and installs every library skill for one harness
// target. A missing/empty library is not an error — it reports the same
// "no skills rendered" the bridge surfaced when symskills produced no
// output for a target. Render/install failures for any library skill
// surface as an error result naming the failing skills.
func syncTarget(ctx context.Context, target render.Target, opts Options, dryRun bool) Result {
	if ctx.Err() != nil {
		return Result{
			Target:  string(target),
			Status:  "error",
			Message: fmt.Sprintf("sync timed out: %v", ctx.Err()),
		}
	}
	entries, err := os.ReadDir(opts.LibraryDir)
	if errors.Is(err, os.ErrNotExist) {
		return Result{Target: string(target), Status: "ok", Message: "no skills rendered"}
	}
	if err != nil {
		return Result{
			Target:  string(target),
			Status:  "error",
			Message: fmt.Sprintf("read skills library: %v", err),
		}
	}
	if len(entries) == 0 {
		return Result{Target: string(target), Status: "ok", Message: "no skills rendered"}
	}

	var done int
	var failed []string
	for _, entry := range entries {
		if ctx.Err() != nil {
			return Result{
				Target:  string(target),
				Status:  "error",
				Message: fmt.Sprintf("sync timed out: %v", ctx.Err()),
			}
		}
		if !entry.IsDir() || strings.HasPrefix(entry.Name(), ".") {
			continue
		}
		root := filepath.Join(opts.LibraryDir, entry.Name())
		if _, err := os.Stat(filepath.Join(root, "SKILL.md")); err != nil {
			continue // not a skill directory
		}

		bundle, err := skill.LoadBundle(root)
		if err != nil {
			failed = append(failed, fmt.Sprintf("%s: load: %v", entry.Name(), err))
			continue
		}
		// A manifest that disables this target is a deliberate opt-out,
		// not a failure: `symskills render --target` counted it too.
		if targetCfg, ok := bundle.Manifest.Targets[string(target)]; ok && !targetCfg.Enabled {
			continue
		}

		if dryRun {
			if err := planSkill(ctx, bundle, target, opts); err != nil {
				failed = append(failed, err.Error())
				continue
			}
			done++
			continue
		}
		if err := installSkill(bundle, target, opts); err != nil {
			failed = append(failed, err.Error())
			continue
		}
		done++
	}

	if ctx.Err() != nil {
		return Result{
			Target:  string(target),
			Status:  "error",
			Message: fmt.Sprintf("sync timed out: %v", ctx.Err()),
		}
	}
	if len(failed) > 0 {
		return Result{
			Target:  string(target),
			Status:  "error",
			Message: strings.Join(failed, "; "),
		}
	}
	if done == 0 {
		return Result{Target: string(target), Status: "ok", Message: "no skills rendered"}
	}
	if dryRun {
		return Result{Target: string(target), Status: "ok", Message: fmt.Sprintf("%d skills planned", done)}
	}
	return Result{Target: string(target), Status: "ok", Message: fmt.Sprintf("%d skills rendered and installed", done)}
}

// planSkill renders a skill in memory for the target and resolves its
// install destination, writing nothing. It is the dry-run equivalent of
// installSkill.
func planSkill(ctx context.Context, bundle *skill.Bundle, target render.Target, opts Options) error {
	if ctx.Err() != nil {
		return fmt.Errorf("%s: %v", bundle.Manifest.Skill.Name, ctx.Err())
	}
	item, err := render.RenderTarget(bundle, target)
	if err != nil {
		return fmt.Errorf("%s: render: %w", bundle.Manifest.Skill.Name, err)
	}
	if _, err := install.InstallPath(target, item.Name, installOptions(opts, bundle)); err != nil {
		return fmt.Errorf("%s: resolve install path: %w", bundle.Manifest.Skill.Name, err)
	}
	return nil
}

// installSkill renders a skill for the target into the render dir and
// installs the result into the harness skill root, mirroring what the
// old `symskills render --target <t>` subprocess did.
func installSkill(bundle *skill.Bundle, target render.Target, opts Options) error {
	rendered, errs := render.RenderAll(bundle, opts.RenderDir, []render.Target{target})
	if len(rendered) == 0 {
		if len(errs) > 0 {
			return fmt.Errorf("%s: render: %w", bundle.Manifest.Skill.Name, errs[0])
		}
		return fmt.Errorf("%s: render: target %s produced no output", bundle.Manifest.Skill.Name, target)
	}
	result, err := install.Install(install.RenderedSkill{
		Target: target,
		Name:   rendered[0].Name,
		Path:   rendered[0].Path,
	}, installOptions(opts, bundle))
	if err != nil {
		return fmt.Errorf("%s: install: %w", bundle.Manifest.Skill.Name, err)
	}
	_ = result
	return nil
}

// installOptions builds the install options for one skill, carrying the
// bundle's executable policy into the harness skill root.
func installOptions(opts Options, bundle *skill.Bundle) install.Options {
	return install.Options{
		HomeDir:         opts.HomeDir,
		ProjectDir:      opts.ProjectDir,
		Scope:           render.ScopeUser,
		BaseDir:         opts.BaseDir,
		AllowExecutable: bundle.Manifest.Skill.AllowExecutable,
	}
}
