// Package render creates harness-specific skill folders from portable bundles.
package render

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"sort"
	"strconv"
	"strings"

	"github.com/BurntSushi/toml"
	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-brain/internal/skills/fsutil"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
	"github.com/danieljustus/symaira-brain/internal/skills/variant"
	"gopkg.in/yaml.v3"
)

// Target is an alias of the registry-owned skill target identity. The
// compatibility constants keep existing skills APIs source-compatible while
// ensuring target names are declared only in internal/harness.
type Target = harness.SkillTarget

const (
	TargetOpenCode    Target = harness.SkillTargetOpenCode
	TargetClaude      Target = harness.SkillTargetClaude
	TargetCodex       Target = harness.SkillTargetCodex
	TargetHermes      Target = harness.SkillTargetHermes
	TargetAntigravity Target = harness.SkillTargetAntigravity
	TargetOpenClaw    Target = harness.SkillTargetOpenClaw
)

// init publishes the target registry to the skill package, which validates
// harness-variant markers but cannot import this package without a cycle.
func init() {
	skill.KnownTargets = func() []string {
		names := make([]string, len(Targets))
		for i, spec := range Targets {
			names[i] = string(spec.Name)
		}
		return names
	}
}

// DefaultTargets returns the list of all registered target names.
func DefaultTargets() []Target {
	targets := make([]Target, len(Targets))
	for i, spec := range Targets {
		targets[i] = spec.Name
	}
	return targets
}

// Scope represents the installation scope for a target harness.
type Scope string

const (
	ScopeUser    Scope = "user"
	ScopeProject Scope = "project"
)

// TargetSpec holds metadata and path functions for a single harness target.
// Targets is the single registry; any code that needs per-target information
// (paths, display names, quirks) should read from here.
type TargetSpec struct {
	Name        Target
	DisplayName string
	BinaryName  string
	ConfigDir   func(home, project string, scope Scope) string
	SkillRoot   func(home, project string, scope Scope) string
	// OverlayDir overrides the overlay directory name (defaults to the
	// target name). Used by user-defined targets.
	OverlayDir string
	// MetadataFile is a relative output path inside the rendered skill
	// (e.g. "agents/openai.yaml") written verbatim from MetadataTemplate.
	MetadataFile     string
	MetadataTemplate string
	Quirks           string
	// Capabilities declares what this harness runtime offers a skill, as
	// capability name -> supported. A name absent from the map is
	// *unknown*, not unsupported: symskills cannot observe a harness
	// runtime, and refusing a render on a guess would be worse than
	// rendering with a warning. Users complete the picture for their own
	// harness builds via [capabilities.<target>] in config.toml.
	//
	// The built-in declarations below are deliberately sparse: only
	// capabilities evidenced by a harness's own documented skill-facing
	// tooling are declared here.
	Capabilities map[string]bool
}

// CustomTargetSpec is the declarative shape for user-defined targets loaded
// from config. Paths may be absolute or relative to home.
type CustomTargetSpec struct {
	Name             string
	DisplayName      string
	BinaryName       string
	SkillRootUser    string
	SkillRootProject string
	MetadataFile     string
	MetadataTemplate string
	OverlayDir       string
	Capabilities     map[string]bool
}

// RegisterCustomTargets appends user-defined targets to the registry. It
// returns an error when a custom name collides with a built-in target or with
// an already-registered custom target. Skill roots must be non-empty.
func RegisterCustomTargets(specs []CustomTargetSpec) error {
	for _, c := range specs {
		name := Target(c.Name)
		if name == "" {
			return fmt.Errorf("custom target: name is required")
		}
		if _, ok := LookupSpec(name); ok {
			return fmt.Errorf("custom target %q collides with an existing target", c.Name)
		}
		if c.SkillRootUser == "" {
			return fmt.Errorf("custom target %q: skill_root_user is required", c.Name)
		}
		userRoot := filepath.Clean(c.SkillRootUser)
		projectRoot := userRoot
		if c.SkillRootProject != "" {
			projectRoot = filepath.Clean(c.SkillRootProject)
		}
		displayName := c.DisplayName
		if displayName == "" {
			displayName = c.Name
		}
		Targets = append(Targets, TargetSpec{
			Name:        name,
			DisplayName: displayName,
			BinaryName:  c.BinaryName,
			ConfigDir: func(home, project string, scope Scope) string {
				if scope == ScopeProject && project != "" {
					return filepath.Dir(projectRoot)
				}
				return filepath.Dir(userRoot)
			},
			SkillRoot: func(home, project string, scope Scope) string {
				if scope == ScopeProject && project != "" {
					return projectRoot
				}
				return userRoot
			},
			OverlayDir:       c.OverlayDir,
			MetadataFile:     c.MetadataFile,
			MetadataTemplate: c.MetadataTemplate,
			Capabilities:     c.Capabilities,
			Quirks:           "User-defined target from config.toml",
		})
	}
	return nil
}

// overlayDir returns the overlay directory name for a target, defaulting to
// the target name.
func overlayDir(target Target) string {
	if spec, ok := LookupSpec(target); ok && spec.OverlayDir != "" {
		return spec.OverlayDir
	}
	return string(target)
}

// builtInTargetSpecs contains target-specific rendering behavior. Harness
// identity and capability membership remain owned by internal/harness.
var builtInTargetSpecs = []TargetSpec{
	{
		Name:        TargetOpenCode,
		DisplayName: "OpenCode",
		BinaryName:  "opencode",
		ConfigDir: func(home, project string, scope Scope) string {
			if scope == ScopeProject && project != "" {
				return filepath.Join(project, ".opencode")
			}
			return filepath.Join(home, ".config", "opencode")
		},
		SkillRoot: func(home, project string, scope Scope) string {
			if scope == ScopeProject && project != "" {
				return filepath.Join(project, ".opencode", "skills")
			}
			return filepath.Join(home, ".config", "opencode", "skills")
		},
	},
	{
		Name:        TargetClaude,
		DisplayName: "Claude Code",
		BinaryName:  "claude",
		// Claude Code exposes an agent-dispatch tool to skills.
		Capabilities: map[string]bool{CapSubagents: true},
		ConfigDir: func(home, project string, scope Scope) string {
			if scope == ScopeProject && project != "" {
				return filepath.Join(project, ".claude")
			}
			return filepath.Join(home, ".claude")
		},
		SkillRoot: func(home, project string, scope Scope) string {
			if scope == ScopeProject && project != "" {
				return filepath.Join(project, ".claude", "skills")
			}
			return filepath.Join(home, ".claude", "skills")
		},
	},
	{
		Name:        TargetCodex,
		DisplayName: "Codex",
		BinaryName:  "codex",
		ConfigDir: func(home, project string, scope Scope) string {
			if scope == ScopeProject && project != "" {
				return filepath.Join(project, ".agents")
			}
			return filepath.Join(home, ".agents")
		},
		SkillRoot: func(home, project string, scope Scope) string {
			if scope == ScopeProject && project != "" {
				return filepath.Join(project, ".agents", "skills")
			}
			return filepath.Join(home, ".agents", "skills")
		},
		Quirks: "Writes agents/openai.yaml metadata on render",
	},
	{
		Name:        TargetHermes,
		DisplayName: "Hermes",
		BinaryName:  "hermes",
		// Hermes exposes delegate_task to skills.
		Capabilities: map[string]bool{CapSubagents: true},
		ConfigDir: func(home, project string, scope Scope) string {
			if scope == ScopeProject && project != "" {
				return filepath.Join(project, ".hermes")
			}
			return filepath.Join(home, ".hermes")
		},
		SkillRoot: func(home, project string, scope Scope) string {
			if scope == ScopeProject && project != "" {
				return filepath.Join(project, ".hermes", "skills")
			}
			return filepath.Join(home, ".hermes", "skills", "symaira")
		},
	},
	{
		Name:        TargetAntigravity,
		DisplayName: "Antigravity",
		BinaryName:  "agy",
		ConfigDir: func(home, project string, scope Scope) string {
			if scope == ScopeProject && project != "" {
				return filepath.Join(project, ".agents")
			}
			return filepath.Join(home, ".gemini", "antigravity-cli")
		},
		SkillRoot: func(home, project string, scope Scope) string {
			if scope == ScopeProject && project != "" {
				return filepath.Join(project, ".agents", "skills")
			}
			return filepath.Join(home, ".gemini", "antigravity-cli", "skills")
		},
		Quirks: "Global skills live in ~/.gemini/antigravity-cli/skills (docs: antigravity.google/docs/skills); workspace skills share <project>/.agents/skills with Codex/OpenClaw",
	},
	{
		Name:        TargetOpenClaw,
		DisplayName: "OpenClaw",
		BinaryName:  "openclaw",
		ConfigDir: func(home, project string, scope Scope) string {
			if scope == ScopeProject && project != "" {
				return filepath.Join(project, ".agents")
			}
			return filepath.Join(home, ".openclaw")
		},
		SkillRoot: func(home, project string, scope Scope) string {
			if scope == ScopeProject && project != "" {
				return filepath.Join(project, ".agents", "skills")
			}
			return filepath.Join(home, ".openclaw", "skills")
		},
		Quirks: "Managed skills load from ~/.openclaw/skills (default state dir, docs: docs.openclaw.ai/tools/skills); also reads ~/.agents/skills and <workspace>/skills",
	},
}

// Targets is derived from the harness registry. Runtime custom targets are
// appended to this slice by RegisterCustomTargets and are never discarded.
var Targets = buildTargets()

func buildTargets() []TargetSpec {
	targets := make([]TargetSpec, 0, len(builtInTargetSpecs))
	for _, h := range harness.All {
		if h.SkillTarget == harness.SkillTargetNone {
			continue
		}
		for _, spec := range builtInTargetSpecs {
			if spec.Name == Target(h.SkillTarget) {
				targets = append(targets, spec)
				break
			}
		}
	}
	return targets
}

// LookupSpec returns the TargetSpec for the given target and a boolean
// indicating whether it was found.
func LookupSpec(t Target) (TargetSpec, bool) {
	for _, spec := range Targets {
		if spec.Name == t {
			return spec, true
		}
	}
	return TargetSpec{}, false
}

// MustLookupSpec returns the TargetSpec for the given target, panicking
// if the target is unknown.
func MustLookupSpec(t Target) TargetSpec {
	spec, ok := LookupSpec(t)
	if !ok {
		panic(fmt.Sprintf("render: unknown target %q", t))
	}
	return spec
}

// RenderMeta carries optional provenance metadata for profile-aware rendering.
type RenderMeta struct {
	Source  string
	Profile string
	Alias   string // profile alias overrides TargetConfig.Alias
	// IgnoreCapabilities renders a skill for a target that declares it
	// lacks a required capability. The result never claims compatibility
	// with that target, and the reason is reported as a warning.
	IgnoreCapabilities bool
}

type Rendered struct {
	Target      Target            `json:"target"`
	Name        string            `json:"name"`
	Path        string            `json:"path,omitempty"`
	Frontmatter skill.Frontmatter `json:"frontmatter"`
	SkillMD     string            `json:"skill_md,omitempty"`
	Source      string            `json:"source,omitempty"`
	Profile     string            `json:"profile,omitempty"`
	// Variants reports what this target changed relative to the canonical
	// source. Nil when the skill uses no blocks and no terms.
	Variants *VariantReport `json:"variants,omitempty"`
	// Files holds resolved markdown resources (relative slash path ->
	// content) that differ from the source because of a block override or
	// a term. Paths absent here travel byte-identical.
	Files map[string]string `json:"-"`
	// Warnings carries non-fatal findings about this render: a required
	// capability the target has not declared, or a requirement overridden
	// with IgnoreCapabilities.
	Warnings []string `json:"warnings,omitempty"`
	// UnmetRequirements lists the required capabilities this target does
	// not satisfy. Non-empty only on a forced render.
	UnmetRequirements []CapabilityGap `json:"unmet_requirements,omitempty"`
}

// VariantReport summarises the harness-specific deltas applied to one target.
type VariantReport struct {
	// Blocks lists the block ids replaced by an overlay override, sorted.
	Blocks []string `json:"blocks,omitempty"`
	// Terms maps each resolved term to the value this target received.
	Terms map[string]string `json:"terms,omitempty"`
	// Files lists the markdown resources whose content changed, sorted.
	Files []string `json:"files,omitempty"`
	// ReplacedBytes and SourceBytes give the per-target divergence: how
	// much of the canonical text this harness replaces.
	ReplacedBytes int `json:"replaced_bytes"`
	SourceBytes   int `json:"source_bytes"`
}

// RenderTarget returns a target-specific SKILL.md without writing files.
func RenderTarget(bundle *skill.Bundle, target Target, meta ...RenderMeta) (Rendered, error) {
	if bundle == nil {
		return Rendered{}, fmt.Errorf("bundle is nil")
	}

	// Reject bundles whose validation errors would make the rendered output
	// misrepresent the source: a traversing overlay reference (#80), a
	// malformed variant marker, an override addressing a block that does
	// not exist, or an unresolvable term. Other validation problems
	// (missing description, empty body, etc.) are reported by
	// `skills_validate` but do not block rendering here.
	for _, issue := range skill.Validate(bundle) {
		if issue.Severity == "error" && skill.IsRenderBlocking(issue.Code) {
			return Rendered{}, fmt.Errorf("validation error: %s", issue.Message)
		}
	}

	cfg, hasCfg := bundle.Manifest.Targets[string(target)]
	if hasCfg && !cfg.Enabled {
		return Rendered{}, fmt.Errorf("target %s is disabled", target)
	}

	var opts RenderMeta
	if len(meta) > 0 {
		opts = meta[0]
	}
	unsupported, unknown, err := checkRequirements(bundle.Manifest.Skill.Requires, target)
	if err != nil {
		return Rendered{}, err
	}
	if len(unsupported) > 0 && !opts.IgnoreCapabilities {
		return Rendered{}, fmt.Errorf(
			"target %s does not support %s required by this skill; disable the target in symskills.toml, or render with --ignore-capabilities to produce output that does not claim compatibility",
			target, describeGaps(unsupported))
	}
	var warnings []string
	if len(unsupported) > 0 {
		warnings = append(warnings, fmt.Sprintf(
			"rendered for %s despite unsupported %s; the result does not declare compatibility with this target",
			target, describeGaps(unsupported)))
	}
	if len(unknown) > 0 {
		warnings = append(warnings, fmt.Sprintf(
			"target %s has not declared %s required by this skill; rendering anyway — record what your harness supports under [capabilities.%s] in config.toml",
			target, describeGaps(unknown), target))
	}

	fm := bundle.Frontmatter
	metadata := map[string]any{}
	for k, v := range fm.Metadata {
		// Metadata namespaced under a harness target name belongs to that
		// target only; shipping metadata.hermes to every other harness was
		// leaking one target's conventions into all the others.
		if _, isForeignTarget := LookupSpec(Target(k)); isForeignTarget && k != string(target) {
			continue
		}
		metadata[k] = v
	}
	for k, v := range cfg.Metadata {
		metadata[k] = v
	}
	fm.Metadata = metadata
	// A render only claims compatibility it can stand behind: a forced
	// render past an unsupported capability declares none.
	fm.Compatibility = string(target)
	if len(unsupported) > 0 {
		fm.Compatibility = ""
	}
	// Alias precedence: profile alias (RenderMeta) > target config alias > manifest name.
	if opts.Alias != "" {
		fm.Name = opts.Alias
	} else if cfg.Alias != "" {
		fm.Name = cfg.Alias
	} else if bundle.Manifest.Skill.Name != "" {
		fm.Name = bundle.Manifest.Skill.Name
	}
	if cfg.Description != "" {
		fm.Description = cfg.Description
	}

	if err := applyFrontmatterOverlay(bundle.Root, target, &fm); err != nil {
		return Rendered{}, err
	}
	if err := skill.ValidateSkillName(fm.Name); err != nil {
		return Rendered{}, fmt.Errorf("invalid resolved name for target %s: %w", target, err)
	}
	composed, err := renderBody(bundle, target, cfg)
	if err != nil {
		return Rendered{}, err
	}
	body, files, report, err := resolveVariants(bundle, target, composed)
	if err != nil {
		return Rendered{}, err
	}
	skillMD, err := encodeSkillMD(fm, body)
	if err != nil {
		return Rendered{}, err
	}
	item := Rendered{
		Target:            target,
		Name:              fm.Name,
		Frontmatter:       fm,
		SkillMD:           skillMD,
		Variants:          report,
		Files:             files,
		Warnings:          warnings,
		UnmetRequirements: unsupported,
		Source:            opts.Source,
		Profile:           opts.Profile,
	}
	return item, nil
}

// describeGaps renders a capability gap list for an error or warning.
func describeGaps(gaps []CapabilityGap) string {
	names := make([]string, len(gaps))
	for i, gap := range gaps {
		names[i] = strconv.Quote(gap.Capability)
	}
	noun := "capability"
	if len(names) > 1 {
		noun = "capabilities"
	}
	return noun + " " + strings.Join(names, ", ")
}

// resolveVariants applies the harness-variant constructs for one target to
// the composed SKILL.md body and to every markdown resource. It returns the
// resolved body, the markdown resources whose content actually changed, and a
// report of what changed. A skill that uses no blocks and no terms gets back
// the input unchanged, an empty file map, and a nil report — the no-op path
// that keeps existing renders byte-identical.
func resolveVariants(bundle *skill.Bundle, target Target, composed string) (string, map[string]string, *VariantReport, error) {
	opts := variant.Options{
		Target:    string(target),
		Overrides: bundle.BlockOverrides[overlayDir(target)],
		Terms:     bundle.Manifest.Terms,
	}

	bodyResult, problems := variant.Apply(composed, opts)
	// The composed body is SKILL.md plus overlay fragments, so a line
	// number in it matches no single file; report the finding without one.
	if err := firstBlockingProblem("composed SKILL.md body", false, problems); err != nil {
		return "", nil, nil, err
	}

	report := &VariantReport{
		Blocks:        append([]string(nil), bodyResult.Blocks...),
		ReplacedBytes: bodyResult.ReplacedBytes,
		SourceBytes:   bodyResult.SourceBytes,
	}
	terms := map[string]string{}
	for name, value := range bodyResult.Terms {
		terms[name] = value
	}

	paths := make([]string, 0, len(bundle.Markdown))
	for path := range bundle.Markdown {
		paths = append(paths, path)
	}
	sort.Strings(paths)

	files := map[string]string{}
	for _, path := range paths {
		source := bundle.Markdown[path]
		result, problems := variant.Apply(source, opts)
		if err := firstBlockingProblem(path, true, problems); err != nil {
			return "", nil, nil, err
		}
		report.SourceBytes += result.SourceBytes
		report.ReplacedBytes += result.ReplacedBytes
		report.Blocks = append(report.Blocks, result.Blocks...)
		for name, value := range result.Terms {
			terms[name] = value
		}
		if result.Text != source {
			files[path] = result.Text
			report.Files = append(report.Files, path)
		}
	}

	sort.Strings(report.Blocks)
	report.Blocks = slices.Compact(report.Blocks)
	if len(terms) > 0 {
		report.Terms = terms
	}
	if len(report.Blocks) == 0 && len(report.Terms) == 0 && len(report.Files) == 0 {
		return bodyResult.Text, map[string]string{}, nil, nil
	}
	return bodyResult.Text, files, report, nil
}

// firstBlockingProblem turns the first render-blocking variant problem into
// an error. Markers inside overlay prepend/append fragments never reach
// skill.Validate, so this is the guard that catches them.
func firstBlockingProblem(path string, withLine bool, problems []variant.Problem) error {
	for _, problem := range problems {
		if problem.Severity != variant.SeverityError || !skill.IsRenderBlocking(problem.Code) {
			continue
		}
		if withLine {
			return fmt.Errorf("%s: %s", path, problem.String())
		}
		return fmt.Errorf("%s: %s", path, problem.Message)
	}
	return nil
}

func renderBody(bundle *skill.Bundle, target Target, cfg skill.TargetConfig) (string, error) {
	prepend, err := overlayText(bundle.Root, target, "prepend.md", cfg.Prepend)
	if err != nil {
		return "", err
	}
	appendText, err := overlayText(bundle.Root, target, "append.md", cfg.Append)
	if err != nil {
		return "", err
	}
	var parts []string
	if strings.TrimSpace(prepend) != "" {
		parts = append(parts, strings.TrimRight(prepend, "\n"))
	}
	parts = append(parts, strings.TrimRight(bundle.Body, "\n"))
	if strings.TrimSpace(appendText) != "" {
		parts = append(parts, strings.TrimRight(appendText, "\n"))
	}
	return strings.Join(parts, "\n\n") + "\n", nil
}

func overlayText(root string, target Target, defaultName, configured string) (string, error) {
	if configured != "" {
		if filepath.IsAbs(configured) {
			return "", fmt.Errorf("overlay reference %q must be relative", configured)
		}
		clean := filepath.Clean(configured)
		if strings.HasPrefix(clean, ".."+string(filepath.Separator)) || clean == ".." {
			return "", fmt.Errorf("overlay reference %q escapes skill root", configured)
		}
		return readOptional(filepath.Join(root, clean))
	}
	return readOptional(filepath.Join(root, "overlays", overlayDir(target), defaultName))
}

func readOptional(path string) (string, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return "", nil
		}
		return "", err
	}
	return string(raw), nil
}

func applyFrontmatterOverlay(root string, target Target, fm *skill.Frontmatter) error {
	path := filepath.Join(root, "overlays", overlayDir(target), "frontmatter.toml")
	if _, err := os.Stat(path); errors.Is(err, os.ErrNotExist) {
		return nil
	} else if err != nil {
		return err
	}
	var raw map[string]any
	if _, err := toml.DecodeFile(path, &raw); err != nil {
		return fmt.Errorf("parse %s: %w", path, err)
	}
	if v, ok := raw["name"].(string); ok && v != "" {
		fm.Name = v
	}
	if v, ok := raw["description"].(string); ok && v != "" {
		fm.Description = v
	}
	if v, ok := raw["compatibility"].(string); ok && v != "" {
		fm.Compatibility = v
	}
	if meta, ok := raw["metadata"].(map[string]any); ok {
		if fm.Metadata == nil {
			fm.Metadata = map[string]any{}
		}
		for k, v := range meta {
			fm.Metadata[k] = v
		}
	}
	return nil
}

func encodeSkillMD(fm skill.Frontmatter, body string) (string, error) {
	data, err := yaml.Marshal(fm)
	if err != nil {
		return "", err
	}
	return "---\n" + string(data) + "---\n\n" + body, nil
}

// RenderAll writes target-specific skill folders under outDir and returns the
// successfully rendered items along with any per-target errors.
func RenderAll(bundle *skill.Bundle, outDir string, targets []Target, meta ...RenderMeta) ([]Rendered, []error) {
	if len(targets) == 0 {
		targets = DefaultTargets()
	}
	// The source tree hash is a per-bundle property; compute it once and
	// reuse it for every target instead of walking the tree per target.
	treeHash := sourceTreeHash(bundle.Root)
	var rendered []Rendered
	var errs []error
	for _, target := range targets {
		item, err := RenderTarget(bundle, target, meta...)
		if err != nil {
			errs = append(errs, fmt.Errorf("target %s: %w", target, err))
			continue
		}
		dst := filepath.Join(outDir, string(target), item.Name)
		if err := writeRendered(bundle.Root, dst, item, target, treeHash); err != nil {
			errs = append(errs, fmt.Errorf("target %s: %w", target, err))
			continue
		}
		item.Path = dst
		rendered = append(rendered, item)
	}
	return rendered, errs
}

// walkBundleDir is the tree walker used by sourceTreeHash. It is a variable
// so tests can count invocations and prove the tree is walked exactly once
// per bundle.
var walkBundleDir = filepath.WalkDir

// stagingMkdirTemp is the temporary-directory factory used by StagingRender.
// It is a variable so tests can observe and control staging directories.
var stagingMkdirTemp = os.MkdirTemp

// StagingRender renders the bundle into a fresh temporary directory and
// returns the rendered items together with a cleanup function that removes
// that directory. Comparison renders (diff, and later sync) must use this
// helper instead of writing into RenderDir, which is only written when a
// result is actually committed: in symlink install mode RenderDir is the
// live installed artifact, and rewriting it would destroy the very state a
// comparison wants to read. The cleanup function must be called (typically
// via defer) on every exit path, including error paths; when no target
// produced output the staging directory is removed before the first render
// error is returned. The source_hash short-circuit never fires in a fresh
// staging directory (no marker present), so staging renders always do full
// work.
func StagingRender(bundle *skill.Bundle, targets []Target, meta ...RenderMeta) ([]Rendered, func(), error) {
	dir, err := stagingMkdirTemp("", "symskills-staging-")
	if err != nil {
		return nil, func() {}, err
	}
	rendered, errs := RenderAll(bundle, dir, targets, meta...)
	if len(rendered) == 0 {
		_ = os.RemoveAll(dir)
		if len(errs) > 0 {
			return nil, func() {}, errs[0]
		}
		return nil, func() {}, fmt.Errorf("no render output for the requested targets")
	}
	return rendered, func() { _ = os.RemoveAll(dir) }, nil
}

// CachedStagingRender reuses a persistent comparison render when the source
// bundle fingerprint and renderer version are unchanged. The cache is never
// used for installs; it only avoids repeating the render pipeline during
// read-only status scans.
func CachedStagingRender(bundle *skill.Bundle, targets []Target, cacheRoot string, meta ...RenderMeta) ([]Rendered, func(), error) {
	if cacheRoot == "" {
		return StagingRender(bundle, targets, meta...)
	}
	if len(targets) == 0 {
		targets = DefaultTargets()
	}
	fingerprint, err := sourceFingerprint(bundle.Root)
	if err != nil {
		return nil, func() {}, err
	}
	const cacheVersion = "status-render-v1"
	var rendered []Rendered
	var errs []error
	for _, target := range targets {
		keyHash := sha256.Sum256([]byte(cacheVersion + "\x00" + bundle.Root + "\x00" + string(target)))
		key := hex.EncodeToString(keyHash[:])
		dst := filepath.Join(cacheRoot, "status-render", key)
		metaPath := filepath.Join(cacheRoot, "status-render", key+".json")
		var cached struct {
			Fingerprint string `json:"fingerprint"`
			Target      Target `json:"target"`
			Name        string `json:"name"`
		}
		data, readErr := os.ReadFile(metaPath)
		if readErr == nil && json.Unmarshal(data, &cached) == nil && cached.Fingerprint == fingerprint && cached.Target == target && cached.Name != "" {
			if _, err := os.Stat(filepath.Join(dst, "SKILL.md")); err == nil {
				rendered = append(rendered, Rendered{Target: target, Name: cached.Name, Path: dst})
				continue
			}
		}
		item, rerr := RenderTarget(bundle, target, meta...)
		if rerr != nil {
			errs = append(errs, fmt.Errorf("target %s: %w", target, rerr))
			continue
		}
		if err := writeRendered(bundle.Root, dst, item, target, fingerprint); err != nil {
			errs = append(errs, fmt.Errorf("target %s: %w", target, err))
			continue
		}
		cacheData, merr := json.Marshal(struct {
			Fingerprint string `json:"fingerprint"`
			Target      Target `json:"target"`
			Name        string `json:"name"`
		}{fingerprint, target, item.Name})
		if merr != nil {
			errs = append(errs, fmt.Errorf("target %s: %w", target, merr))
			continue
		}
		if err := os.WriteFile(metaPath, append(cacheData, '\n'), 0o644); err != nil {
			errs = append(errs, fmt.Errorf("target %s: %w", target, err))
			continue
		}
		item.Path = dst
		rendered = append(rendered, item)
	}
	if len(rendered) == 0 {
		if len(errs) > 0 {
			return nil, func() {}, errs[0]
		}
		return nil, func() {}, fmt.Errorf("no render output for the requested targets")
	}
	return rendered, func() {}, nil
}

// sourceFingerprint hashes all source files, including overlays and the
// canonical SKILL.md, so cached comparison output is invalidated by any
// input that can affect rendering.
func sourceFingerprint(root string) (string, error) {
	h := sha256.New()
	err := filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			if d.Name() == ".git" {
				return filepath.SkipDir
			}
			return nil
		}
		rel, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		h.Write([]byte(rel))
		h.Write([]byte{0})
		h.Write(data)
		h.Write([]byte{0})
		return nil
	})
	if err != nil {
		return "", err
	}
	return hex.EncodeToString(h.Sum(nil)), nil
}

// sourceTreeHash computes a content hash of the source tree (support files
// only; SKILL.md and symskills.toml are part of the rendered body). It is
// expensive — it reads every support file — so callers must compute it once
// per bundle and reuse the result across targets.
func sourceTreeHash(bundleRoot string) string {
	h := sha256.New()
	_ = walkBundleDir(bundleRoot, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			if d.Name() == ".git" || d.Name() == "overlays" {
				return filepath.SkipDir
			}
			return nil
		}
		rel, err := filepath.Rel(bundleRoot, path)
		if err != nil {
			return err
		}
		if rel == "SKILL.md" || rel == "symskills.toml" {
			return nil
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		h.Write([]byte(rel))
		h.Write([]byte{0})
		h.Write(data)
		h.Write([]byte{0})
		return nil
	})
	return hex.EncodeToString(h.Sum(nil))
}

// sourceHash combines the once-per-bundle source tree hash with the
// per-target rendered SKILL.md content and target name so re-renders with
// unchanged input can be skipped. The tree hash covers the raw support
// files, so resolved markdown resources — whose content depends on this
// target's block overrides and terms — are mixed in separately. That mix is
// skipped when the skill resolves no variants, keeping the hash of an
// ordinary skill exactly what it was before harness variants existed.
func sourceHash(treeHash, renderedSkillMD string, target Target, files map[string]string) string {
	h := sha256.New()
	h.Write([]byte(treeHash))
	h.Write([]byte{0})
	h.Write([]byte(renderedSkillMD))
	h.Write([]byte{0})
	h.Write([]byte(target))
	if vh := variantFilesHash(files); vh != "" {
		h.Write([]byte{0})
		h.Write([]byte(vh))
	}
	return hex.EncodeToString(h.Sum(nil))
}

// variantFilesHash hashes the resolved markdown resources in path order. An
// empty map yields an empty hash so it contributes nothing.
func variantFilesHash(files map[string]string) string {
	if len(files) == 0 {
		return ""
	}
	paths := make([]string, 0, len(files))
	for path := range files {
		paths = append(paths, path)
	}
	sort.Strings(paths)
	h := sha256.New()
	for _, path := range paths {
		h.Write([]byte(path))
		h.Write([]byte{0})
		h.Write([]byte(files[path]))
		h.Write([]byte{0})
	}
	return hex.EncodeToString(h.Sum(nil))
}

func writeRendered(root, dst string, item Rendered, target Target, treeHash string) error {
	sh := sourceHash(treeHash, item.SkillMD, target, item.Files)

	// Read any existing marker so we can preserve install-time fields and
	// check whether the output is already current.
	markerPath := filepath.Join(dst, ".symskills.json")
	var existingMarker map[string]interface{}
	if data, err := os.ReadFile(markerPath); err == nil {
		_ = json.Unmarshal(data, &existingMarker)
	}

	// Skip the rewrite when the source hasn't changed.
	if existingMarker != nil {
		if storedHash, ok := existingMarker["source_hash"].(string); ok && storedHash == sh {
			return nil
		}
	}

	// Build the updated marker, preserving any pre-existing fields.
	if existingMarker == nil {
		existingMarker = make(map[string]interface{})
	}
	existingMarker["source_hash"] = sh
	markerBytes, err := json.MarshalIndent(existingMarker, "", "  ")
	if err != nil {
		return err
	}
	markerBytes = append(markerBytes, '\n')

	if err := os.RemoveAll(dst); err != nil {
		return err
	}
	if err := copySupportFiles(root, dst); err != nil {
		return err
	}
	if err := writeResolvedFiles(dst, item.Files); err != nil {
		return err
	}
	if err := os.WriteFile(filepath.Join(dst, "SKILL.md"), []byte(item.SkillMD), 0o644); err != nil {
		return err
	}
	if err := writeTargetMetadata(dst, target, item); err != nil {
		return err
	}
	if err := os.WriteFile(markerPath, markerBytes, 0o644); err != nil {
		return err
	}
	return nil
}

// writeResolvedFiles overwrites the copied markdown resources whose content
// this target resolves differently. copySupportFiles has already placed the
// canonical bytes, so only genuinely changed paths are rewritten.
func writeResolvedFiles(dst string, files map[string]string) error {
	paths := make([]string, 0, len(files))
	for path := range files {
		paths = append(paths, path)
	}
	sort.Strings(paths)
	for _, path := range paths {
		target := filepath.Join(dst, filepath.FromSlash(path))
		if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
			return err
		}
		if err := os.WriteFile(target, []byte(files[path]), 0o644); err != nil {
			return err
		}
	}
	return nil
}

func copySupportFiles(src, dst string) error {
	return fsutil.CopyTree(src, dst, func(rel string, d os.DirEntry) bool {
		if d.IsDir() && (d.Name() == ".git" || d.Name() == "overlays") {
			return true
		}
		return rel == "SKILL.md" || rel == "symskills.toml"
	})
}

func writeCodexMetadata(dst string, item Rendered) error {
	content := fmt.Sprintf(`interface:
  display_name: %q
  short_description: %q
policy:
  allow_implicit_invocation: true
`, item.Name, item.Frontmatter.Description)
	path := filepath.Join(dst, "agents", "openai.yaml")
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	return os.WriteFile(path, []byte(content), 0o644)
}

// writeTargetMetadata writes per-target metadata files into a rendered skill.
// Built-in targets with a fixed metadata contract (Codex) keep their
// generated file; user-defined targets may declare a metadata file whose
// content is taken verbatim from a template file.
func writeTargetMetadata(dst string, target Target, item Rendered) error {
	if target == TargetCodex {
		return writeCodexMetadata(dst, item)
	}
	spec, ok := LookupSpec(target)
	if !ok || spec.MetadataFile == "" {
		return nil
	}
	if spec.MetadataTemplate == "" {
		return fmt.Errorf("target %s: metadata_file %q requires metadata_template", target, spec.MetadataFile)
	}
	content, err := os.ReadFile(spec.MetadataTemplate)
	if err != nil {
		return fmt.Errorf("target %s: read metadata template: %w", target, err)
	}
	path := filepath.Join(dst, spec.MetadataFile)
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	return os.WriteFile(path, content, 0o644)
}

// ParseTarget converts a user-facing target string.
func ParseTarget(s string) (Target, error) {
	for _, spec := range Targets {
		if string(spec.Name) == s {
			return Target(s), nil
		}
	}
	valid := make([]string, 0, len(Targets)+1)
	valid = append(valid, "all")
	for _, spec := range Targets {
		valid = append(valid, string(spec.Name))
	}
	return "", fmt.Errorf("unknown target %q (valid: %s)", s, strings.Join(valid, ", "))
}
