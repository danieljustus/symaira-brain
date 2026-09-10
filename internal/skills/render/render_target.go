package render

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"sort"
	"strconv"
	"strings"

	"github.com/BurntSushi/toml"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
	"github.com/danieljustus/symaira-brain/internal/skills/variant"
	"gopkg.in/yaml.v3"
)

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
	// MetadataFile and MetadataBytes hold one safe, bounded snapshot of a
	// custom target template. Materialization must write this exact snapshot;
	// reopening the ambient template path would permit TOCTOU drift.
	MetadataFile  string `json:"-"`
	MetadataBytes []byte `json:"-"`
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
	if _, ok := LookupSpec(target); !ok {
		return Rendered{}, fmt.Errorf("unknown target %q", target)
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

	if err := applyFrontmatterOverlay(bundle, target, &fm); err != nil {
		return Rendered{}, err
	}
	// A forced render must never regain compatibility through a target overlay.
	if len(unsupported) > 0 {
		fm.Compatibility = ""
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
	metadataFile, metadataBytes, err := targetMetadata(target)
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
		MetadataFile:      metadataFile,
		MetadataBytes:     metadataBytes,
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

func targetMetadata(target Target) (string, []byte, error) {
	if target == TargetCodex {
		return "", nil, nil
	}
	spec, ok := LookupSpec(target)
	if !ok || spec.MetadataFile == "" {
		return "", nil, nil
	}
	if spec.MetadataTemplate == "" {
		return "", nil, fmt.Errorf("target %s: metadata_file %q requires metadata_template", target, spec.MetadataFile)
	}
	clean := filepath.Clean(filepath.FromSlash(spec.MetadataFile))
	if filepath.IsAbs(spec.MetadataFile) || clean == "." || clean == ".." || strings.HasPrefix(clean, ".."+string(filepath.Separator)) {
		return "", nil, fmt.Errorf("target %s: metadata_file %q must be a relative path", target, spec.MetadataFile)
	}
	data, err := skill.ReadExternalBytes(spec.MetadataTemplate, skill.MaxInputSize)
	if err != nil {
		return "", nil, fmt.Errorf("target %s: read metadata template: %w", target, err)
	}
	return filepath.ToSlash(clean), append([]byte(nil), data...), nil
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
	prepend, err := overlayText(bundle, target, "prepend.md", cfg.Prepend)
	if err != nil {
		return "", err
	}
	appendText, err := overlayText(bundle, target, "append.md", cfg.Append)
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

func overlayText(bundle *skill.Bundle, target Target, defaultName, configured string) (string, error) {
	rel := configured
	if rel == "" {
		rel = filepath.ToSlash(filepath.Join("overlays", overlayDir(target), defaultName))
	} else {
		if filepath.IsAbs(rel) {
			return "", fmt.Errorf("overlay reference %q must be relative", configured)
		}
		clean := filepath.Clean(rel)
		if strings.HasPrefix(clean, ".."+string(filepath.Separator)) || clean == ".." {
			return "", fmt.Errorf("overlay reference %q escapes skill root", configured)
		}
		rel = filepath.ToSlash(clean)
	}
	raw, err := skill.ReadBundleText(bundle, rel, skill.MaxInputSize)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return "", nil
		}
		return "", err
	}
	return raw, nil
}

func applyFrontmatterOverlay(bundle *skill.Bundle, target Target, fm *skill.Frontmatter) error {
	rel := filepath.ToSlash(filepath.Join("overlays", overlayDir(target), "frontmatter.toml"))
	data, err := skill.ReadBundleText(bundle, rel, skill.MaxInputSize)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return err
	}
	var raw map[string]any
	if _, err := toml.Decode(data, &raw); err != nil {
		return fmt.Errorf("parse %s: %w", rel, err)
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
