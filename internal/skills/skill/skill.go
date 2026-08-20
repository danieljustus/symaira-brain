// Package skill loads, validates, and imports portable Agent Skill bundles.
package skill

import (
	"bufio"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strings"

	"github.com/BurntSushi/toml"
	"github.com/danieljustus/symaira-brain/internal/skills/fsutil"
	"github.com/danieljustus/symaira-brain/internal/skills/variant"
	"github.com/danieljustus/symaira-brain/internal/skills/vcs"
	"gopkg.in/yaml.v3"
)

// skillNamePattern follows the agentskills specification: lowercase
// alphanumeric segments joined by single hyphens — no consecutive, leading,
// or trailing hyphens (e.g. "pdf--processing" and "pdf-" are invalid).
var skillNamePattern = regexp.MustCompile(`^[a-z0-9]+(?:-[a-z0-9]+)*$`)

// Size limits for portable skill bundles, fixed as a contract following the
// agentskills specification rather than tuned per install.
const (
	// MaxNameLength caps the skill name in characters.
	MaxNameLength = 64
	// MaxDescriptionLength caps the frontmatter description in characters.
	MaxDescriptionLength = 1024
	// MaxBodyLength caps the SKILL.md markdown body in characters.
	MaxBodyLength = 50000
	// MaxResourceSize caps a single resource file in bytes. Executables only
	// materialise under this size (reference implementation contract).
	MaxResourceSize = 10 << 20 // 10 MiB
)

// Frontmatter is the portable SKILL.md metadata symskills understands.
type Frontmatter struct {
	Name                         string         `yaml:"name" json:"name"`
	Description                  string         `yaml:"description" json:"description"`
	Category                     string         `yaml:"category,omitempty" json:"category,omitempty"`
	Version                      string         `yaml:"version,omitempty" json:"version,omitempty"`
	Author                       string         `yaml:"author,omitempty" json:"author,omitempty"`
	License                      string         `yaml:"license,omitempty" json:"license,omitempty"`
	Compatibility                string         `yaml:"compatibility,omitempty" json:"compatibility,omitempty"`
	Platforms                    []string       `yaml:"platforms,omitempty" json:"platforms,omitempty"`
	RequiredEnvironmentVariables []string       `yaml:"required_environment_variables,omitempty" json:"required_environment_variables,omitempty"`
	Metadata                     map[string]any `yaml:"metadata,omitempty" json:"metadata,omitempty"`
}

// Manifest describes symskills-specific SSOT settings.
type Manifest struct {
	Skill   ManifestSkill           `toml:"skill" json:"skill"`
	Targets map[string]TargetConfig `toml:"targets" json:"targets"`
	// Terms is the harness-term table: term name -> ("default"|target) ->
	// value. A {{term:name}} placeholder in SKILL.md or a markdown
	// reference resolves against it. Every term must carry a "default" so
	// the canonical source states something true and harness-neutral.
	Terms map[string]map[string]string `toml:"terms" json:"terms,omitempty"`
}

// ManifestSkill contains portable source metadata.
type ManifestSkill struct {
	Name    string `toml:"name" json:"name"`
	Version string `toml:"version" json:"version"`
	Source  string `toml:"source" json:"source"`
	// AllowExecutable preserves executable bits on resource files during
	// install instead of stripping them (default policy).
	AllowExecutable bool `toml:"allow_executable" json:"allow_executable"`
	// Requires names the harness capabilities this skill needs to work at
	// all. A target that declares it lacks one refuses to render, so a
	// harness never receives a skill it cannot execute. A capability the
	// target has not declared renders with a warning: undeclared is
	// missing information, not evidence of absence.
	Requires []string `toml:"requires" json:"requires,omitempty"`
}

// TargetConfig controls rendering and installation for one harness target.
type TargetConfig struct {
	Enabled     bool              `toml:"enabled" json:"enabled"`
	Alias       string            `toml:"alias" json:"alias"`
	Description string            `toml:"description" json:"description"`
	Scope       string            `toml:"scope" json:"scope"`
	Category    string            `toml:"category" json:"category"`
	Prepend     string            `toml:"prepend" json:"prepend"`
	Append      string            `toml:"append" json:"append"`
	Metadata    map[string]string `toml:"metadata" json:"metadata"`
}

// Resource describes one non-SKILL.md file in a skill bundle. Path is
// relative to the bundle root using forward slashes.
type Resource struct {
	Path       string `json:"path"`
	Size       int64  `json:"size"`
	Mode       string `json:"mode"`
	Executable bool   `json:"executable"`
}

// Bundle is a loaded skill directory.
type Bundle struct {
	Root        string      `json:"root"`
	Frontmatter Frontmatter `json:"frontmatter"`
	Manifest    Manifest    `json:"manifest"`
	Body        string      `json:"body"`
	// Resources inventories every non-SKILL.md file in the bundle (relative
	// path, size, mode, executable flag). Symlinked directories are not
	// descended into; symlinked files are resolved to their target.
	Resources []Resource `json:"resources"`
	// Markdown holds every markdown resource outside overlays/ as
	// relative slash path -> content. It is read once at load so both
	// validation and per-target rendering resolve blocks and terms in
	// references without walking the tree again.
	Markdown map[string]string `json:"-"`
	// BlockOverrides holds the per-target block replacements found under
	// overlays/<dir>/blocks/<id>.md, as overlay directory -> block id ->
	// replacement text.
	BlockOverrides map[string]map[string]string `json:"-"`
	// BodyLineOffset is the number of SKILL.md lines the frontmatter
	// occupies, so a finding in Body can be reported at its real line in
	// the file rather than at its offset within the body.
	BodyLineOffset int `json:"-"`
}

// KnownTargets reports the harness target names this binary supports. It is
// populated by internal/render, which owns the target registry and cannot be
// imported here without an import cycle. A nil hook disables every check that
// needs the registry rather than guessing at it.
var KnownTargets func() []string

func knownTargets() []string {
	if KnownTargets == nil {
		return nil
	}
	return KnownTargets()
}

// Issue is one validation finding.
type Issue struct {
	Code     string `json:"code"`
	Severity string `json:"severity"`
	Message  string `json:"message"`
	Path     string `json:"path,omitempty"`
}

// ImportResult describes an imported bundle.
type ImportResult struct {
	Name string `json:"name"`
	Path string `json:"path"`
	// VCSWarning carries a non-fatal per-skill versioning problem (for
	// example a git failure after the copy succeeded). The import itself
	// succeeded; only the versioning is degraded.
	VCSWarning string `json:"vcs_warning,omitempty"`
}

// BatchImportStatus is the outcome of one batch-imported skill.
type BatchImportStatus string

const (
	BatchImported BatchImportStatus = "imported"
	BatchSkipped  BatchImportStatus = "skipped"
	BatchFailed   BatchImportStatus = "failed"
)

// BatchImportResult describes one item in a batch import.
type BatchImportResult struct {
	Name   string            `json:"name"`
	Path   string            `json:"path,omitempty"`
	Status BatchImportStatus `json:"status"`
	Error  string            `json:"error,omitempty"`
	// VCSWarning mirrors ImportResult.VCSWarning for batch items.
	VCSWarning string `json:"vcs_warning,omitempty"`
}

// LoadBundle reads SKILL.md and optional symskills.toml from a skill root.
func LoadBundle(root string) (*Bundle, error) {
	abs, err := filepath.Abs(root)
	if err != nil {
		return nil, err
	}
	raw, err := os.ReadFile(filepath.Join(abs, "SKILL.md"))
	if err != nil {
		return nil, fmt.Errorf("read SKILL.md: %w", err)
	}
	fm, body, err := parseSkillMD(raw)
	if err != nil {
		return nil, err
	}

	manifest := Manifest{Targets: map[string]TargetConfig{}}
	manifestPath := filepath.Join(abs, "symskills.toml")
	if _, err := os.Stat(manifestPath); err == nil {
		if _, err := toml.DecodeFile(manifestPath, &manifest); err != nil {
			return nil, fmt.Errorf("parse symskills.toml: %w", err)
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	if manifest.Targets == nil {
		manifest.Targets = map[string]TargetConfig{}
	}
	if manifest.Skill.Name == "" {
		manifest.Skill.Name = fm.Name
	}
	if manifest.Skill.Version == "" {
		manifest.Skill.Version = fm.Version
	}

	resources, err := loadResources(abs)
	if err != nil {
		return nil, fmt.Errorf("inventory bundle resources: %w", err)
	}
	markdown, err := loadMarkdown(abs, resources)
	if err != nil {
		return nil, fmt.Errorf("read bundle markdown: %w", err)
	}
	overrides, err := loadBlockOverrides(abs)
	if err != nil {
		return nil, fmt.Errorf("read overlay blocks: %w", err)
	}

	// parseSkillMD returns the body as a suffix of the normalised file, so
	// the lines before it are exactly the frontmatter.
	normalized := strings.ReplaceAll(string(raw), "\r\n", "\n")
	bodyOffset := strings.Count(normalized[:len(normalized)-len(body)], "\n")

	return &Bundle{
		Root:           abs,
		Frontmatter:    fm,
		Manifest:       manifest,
		Body:           body,
		Resources:      resources,
		Markdown:       markdown,
		BlockOverrides: overrides,
		BodyLineOffset: bodyOffset,
	}, nil
}

// IsMarkdown reports whether a bundle-relative path is a markdown resource
// that participates in block and term resolution. Only markdown is resolved:
// scripts and data files travel byte-identical, so a placeholder-looking
// string in a script is never rewritten.
func IsMarkdown(rel string) bool {
	lower := strings.ToLower(rel)
	return strings.HasSuffix(lower, ".md") || strings.HasSuffix(lower, ".markdown")
}

// isOverlayPath reports whether a bundle-relative path lives under overlays/,
// which is render input rather than shipped content.
func isOverlayPath(rel string) bool {
	return rel == "overlays" || strings.HasPrefix(rel, "overlays/")
}

// loadMarkdown reads every markdown resource outside overlays/ into memory.
func loadMarkdown(root string, resources []Resource) (map[string]string, error) {
	out := map[string]string{}
	for _, res := range resources {
		if isOverlayPath(res.Path) || !IsMarkdown(res.Path) {
			continue
		}
		data, err := os.ReadFile(filepath.Join(root, filepath.FromSlash(res.Path)))
		if err != nil {
			return nil, fmt.Errorf("%s: %w", res.Path, err)
		}
		out[res.Path] = string(data)
	}
	return out, nil
}

// loadBlockOverrides reads overlays/<dir>/blocks/<id>.md for every overlay
// directory. A missing overlays/ tree yields an empty map, not an error.
func loadBlockOverrides(root string) (map[string]map[string]string, error) {
	overlaysDir := filepath.Join(root, "overlays")
	entries, err := os.ReadDir(overlaysDir)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return map[string]map[string]string{}, nil
		}
		return nil, err
	}
	out := map[string]map[string]string{}
	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		blocksDir := filepath.Join(overlaysDir, entry.Name(), variant.BlocksDir)
		files, err := os.ReadDir(blocksDir)
		if err != nil {
			if errors.Is(err, os.ErrNotExist) {
				continue
			}
			return nil, err
		}
		blocks := map[string]string{}
		for _, file := range files {
			if file.IsDir() || !IsMarkdown(file.Name()) {
				continue
			}
			data, err := os.ReadFile(filepath.Join(blocksDir, file.Name()))
			if err != nil {
				return nil, err
			}
			id := strings.TrimSuffix(strings.TrimSuffix(file.Name(), ".markdown"), ".md")
			blocks[id] = string(data)
		}
		if len(blocks) > 0 {
			out[entry.Name()] = blocks
		}
	}
	return out, nil
}

// loadResources inventories every non-SKILL.md file under root: relative
// path, size, permission mode, and executable flag. WalkDir order is
// lexical, so the result is deterministic. .git directories are skipped to
// match ImportSkill's copy semantics. Symlinked directories are not
// descended into; symlinked files are resolved to their target so the
// inventory reflects what install would materialize.
func loadResources(root string) ([]Resource, error) {
	resources := []Resource{}
	err := filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		if rel == "." {
			return nil
		}
		if d.IsDir() {
			if d.Name() == ".git" {
				return filepath.SkipDir
			}
			return nil
		}
		if rel == "SKILL.md" {
			return nil
		}
		info, err := d.Info()
		if err != nil {
			return err
		}
		size := info.Size()
		perm := info.Mode().Perm()
		executable := perm&0o111 != 0
		if d.Type()&os.ModeSymlink != 0 {
			// Resolve symlinked files to their target; a dangling or
			// directory-targeted link is skipped.
			target, err := os.Stat(path)
			if err != nil || target.IsDir() {
				return nil
			}
			size = target.Size()
			perm = target.Mode().Perm()
			executable = perm&0o111 != 0
		}
		resources = append(resources, Resource{
			Path:       filepath.ToSlash(rel),
			Size:       size,
			Mode:       fmt.Sprintf("%04o", perm),
			Executable: executable,
		})
		return nil
	})
	return resources, err
}

// UnmarshalYAML accepts scalar values and YAML string lists for fields that
// are represented as strings in the public Frontmatter type.
func (fm *Frontmatter) UnmarshalYAML(value *yaml.Node) error {
	var fields map[string]yaml.Node
	if err := value.Decode(&fields); err != nil {
		return err
	}

	normalized := make(map[string]any, len(fields))
	for name, node := range fields {
		if isStringFrontmatterField(name) {
			text, err := decodeStringFrontmatterField(name, node)
			if err != nil {
				return err
			}
			normalized[name] = text
			continue
		}
		var field any
		if err := node.Decode(&field); err != nil {
			return fmt.Errorf("frontmatter field %q: %w", name, err)
		}
		normalized[name] = field
	}

	data, err := yaml.Marshal(normalized)
	if err != nil {
		return fmt.Errorf("normalize frontmatter: %w", err)
	}
	type frontmatter Frontmatter
	var decoded frontmatter
	if err := yaml.Unmarshal(data, &decoded); err != nil {
		return err
	}
	*fm = Frontmatter(decoded)
	return nil
}

func isStringFrontmatterField(name string) bool {
	switch name {
	case "name", "description", "category", "version", "author", "license", "compatibility":
		return true
	default:
		return false
	}
}

func decodeStringFrontmatterField(name string, node yaml.Node) (string, error) {
	if node.Kind == 0 || node.Tag == "!!null" {
		return "", nil
	}
	if node.Kind == yaml.ScalarNode {
		var text string
		if err := node.Decode(&text); err != nil {
			return "", fmt.Errorf("frontmatter field %q must be a string or a list of strings: %w", name, err)
		}
		return text, nil
	}
	if node.Kind == yaml.SequenceNode {
		var values []string
		if err := node.Decode(&values); err != nil {
			return "", fmt.Errorf("frontmatter field %q must be a string or a list of strings: %w", name, err)
		}
		return strings.Join(values, ", "), nil
	}
	return "", fmt.Errorf("frontmatter field %q must be a string or a list of strings", name)
}

func parseSkillMD(raw []byte) (Frontmatter, string, error) {
	text := strings.ReplaceAll(string(raw), "\r\n", "\n")
	if !strings.HasPrefix(text, "---\n") {
		return Frontmatter{}, "", fmt.Errorf("SKILL.md must start with YAML frontmatter")
	}
	rest := strings.TrimPrefix(text, "---\n")
	end := strings.Index(rest, "\n---")
	if end < 0 {
		return Frontmatter{}, "", fmt.Errorf("SKILL.md frontmatter is not closed")
	}
	fmText := rest[:end]
	body := rest[end+len("\n---"):]
	body = strings.TrimLeft(body, "\n")

	var fm Frontmatter
	if err := yaml.Unmarshal([]byte(fmText), &fm); err != nil {
		return Frontmatter{}, "", fmt.Errorf("parse SKILL.md frontmatter: %w", err)
	}
	if fm.Metadata == nil {
		fm.Metadata = map[string]any{}
	}
	fm.Category = NormalizeCategory(fm.Category)
	return fm, body, nil
}

// NormalizeCategory trims and collapses whitespace without changing the
// display spelling. Library-level canonicalization is handled by
// NormalizeCategories, which can snap case variants to one existing spelling.
func NormalizeCategory(category string) string {
	return strings.Join(strings.Fields(category), " ")
}

// NormalizeCategories canonicalizes category spelling across a loaded library.
// The first non-empty spelling wins; later case-insensitive variants reuse it.
func NormalizeCategories(bundles []*Bundle) {
	canonical := map[string]string{}
	for _, bundle := range bundles {
		if bundle == nil {
			continue
		}
		category := NormalizeCategory(bundle.Frontmatter.Category)
		if category == "" {
			bundle.Frontmatter.Category = ""
			continue
		}
		key := strings.ToLower(category)
		if existing, ok := canonical[key]; ok {
			bundle.Frontmatter.Category = existing
			continue
		}
		canonical[key] = category
		bundle.Frontmatter.Category = category
	}
}

// loadFrontmatterOnly reads SKILL.md's YAML frontmatter without allocating
// the (often much larger) Markdown body, for callers that only need the
// header metadata (e.g. ListLibrary).
func loadFrontmatterOnly(root string) (Frontmatter, error) {
	abs, err := filepath.Abs(root)
	if err != nil {
		return Frontmatter{}, err
	}
	f, err := os.Open(filepath.Join(abs, "SKILL.md"))
	if err != nil {
		return Frontmatter{}, fmt.Errorf("read SKILL.md: %w", err)
	}
	defer f.Close()

	scanner := bufio.NewScanner(f)
	if !scanner.Scan() || scanner.Text() != "---" {
		return Frontmatter{}, fmt.Errorf("SKILL.md must start with YAML frontmatter")
	}

	var fmText strings.Builder
	closed := false
	for scanner.Scan() {
		line := scanner.Text()
		if strings.HasPrefix(line, "---") {
			closed = true
			break
		}
		fmText.WriteString(line)
		fmText.WriteByte('\n')
	}
	if err := scanner.Err(); err != nil {
		return Frontmatter{}, fmt.Errorf("read SKILL.md: %w", err)
	}
	if !closed {
		return Frontmatter{}, fmt.Errorf("SKILL.md frontmatter is not closed")
	}

	var fm Frontmatter
	if err := yaml.Unmarshal([]byte(fmText.String()), &fm); err != nil {
		return Frontmatter{}, fmt.Errorf("parse SKILL.md frontmatter: %w", err)
	}
	if fm.Metadata == nil {
		fm.Metadata = map[string]any{}
	}
	fm.Category = NormalizeCategory(fm.Category)
	return fm, nil
}

func ValidateSkillName(name string) error {
	if strings.TrimSpace(name) == "" {
		return fmt.Errorf("skill name is required")
	}
	if len(name) > MaxNameLength {
		return fmt.Errorf("skill name %q exceeds maximum length of %d characters", name, MaxNameLength)
	}
	if !skillNamePattern.MatchString(name) {
		return fmt.Errorf("skill name %q must be a single lowercase alphanumeric-dash segment without consecutive, leading, or trailing hyphens", name)
	}
	return nil
}

// Validate returns non-fatal validation issues for a loaded bundle.
func Validate(bundle *Bundle) []Issue {
	if bundle == nil {
		return []Issue{{Code: "bundle_required", Severity: "error", Message: "bundle is nil"}}
	}
	var issues []Issue
	name := bundle.Frontmatter.Name
	if strings.TrimSpace(name) == "" {
		issues = append(issues, Issue{Code: "name_required", Severity: "error", Message: "frontmatter name is required", Path: "SKILL.md"})
	} else {
		if len(name) > MaxNameLength {
			issues = append(issues, Issue{Code: "name_too_long", Severity: "error", Message: fmt.Sprintf("name exceeds maximum length of %d characters (actual: %d)", MaxNameLength, len(name)), Path: "SKILL.md"})
		}
		if !skillNamePattern.MatchString(name) {
			issues = append(issues, Issue{Code: "name_format", Severity: "error", Message: "name must use lowercase letters, numbers, and single dashes (no consecutive, leading, or trailing dashes)", Path: "SKILL.md"})
		}
		// The agentskills specification requires the skill name to match the
		// parent directory name, which is how the library layout stores
		// bundles (library/<name>).
		if filepath.Base(bundle.Root) != name {
			issues = append(issues, Issue{Code: "name_dir_mismatch", Severity: "error", Message: fmt.Sprintf("frontmatter name %q must match the parent directory name %q", name, filepath.Base(bundle.Root)), Path: "SKILL.md"})
		}
	}
	if strings.TrimSpace(bundle.Frontmatter.Description) == "" {
		issues = append(issues, Issue{Code: "description_required", Severity: "error", Message: "frontmatter description is required", Path: "SKILL.md"})
	} else if len(bundle.Frontmatter.Description) > MaxDescriptionLength {
		issues = append(issues, Issue{Code: "description_too_long", Severity: "error", Message: fmt.Sprintf("frontmatter description exceeds maximum length of %d characters (actual: %d)", MaxDescriptionLength, len(bundle.Frontmatter.Description)), Path: "SKILL.md"})
	}
	if strings.TrimSpace(bundle.Frontmatter.Category) == "" {
		issues = append(issues, Issue{Code: "category_required", Severity: "warning", Message: "frontmatter category is required; reuse an existing category when possible", Path: "SKILL.md"})
	}
	if strings.TrimSpace(bundle.Body) == "" {
		issues = append(issues, Issue{Code: "body_required", Severity: "error", Message: "SKILL.md body is empty", Path: "SKILL.md"})
	} else if len(bundle.Body) > MaxBodyLength {
		issues = append(issues, Issue{Code: "body_too_long", Severity: "warning", Message: fmt.Sprintf("SKILL.md body exceeds maximum length of %d characters (actual: %d)", MaxBodyLength, len(bundle.Body)), Path: "SKILL.md"})
	}
	for _, res := range bundle.Resources {
		if res.Executable {
			issues = append(issues, Issue{Code: "resource_executable", Severity: "warning", Message: "resource file is executable; install strips the executable bit unless --allow-executable (or the manifest setting) is set", Path: res.Path})
		}
		if res.Size > MaxResourceSize {
			issues = append(issues, Issue{Code: "resource_too_large", Severity: "warning", Message: fmt.Sprintf("resource exceeds maximum size of %d bytes (actual: %d)", MaxResourceSize, res.Size), Path: res.Path})
		}
	}
	for target, cfg := range bundle.Manifest.Targets {
		if !cfg.Enabled {
			continue
		}
		for _, rel := range []string{cfg.Prepend, cfg.Append} {
			if rel == "" {
				continue
			}
			if err := safeRelativeFile(bundle.Root, rel); err != nil {
				issues = append(issues, Issue{Code: "overlay_reference_missing", Severity: "error", Message: err.Error(), Path: "symskills.toml:" + target})
			}
		}
	}
	issues = append(issues, validateVariants(bundle)...)
	return issues
}

// renderBlockingCodes lists the validation codes that make a render refuse
// rather than emit output. They share one property: the rendered result
// would silently misrepresent the source — a traversing overlay reference, a
// malformed marker that would ship as literal HTML, an override addressing a
// block that does not exist, or a placeholder that cannot resolve. Every
// other error-severity code is reported by `validate` and still renders.
var renderBlockingCodes = map[string]bool{
	"overlay_reference_missing":     true,
	variant.CodeMarkerMalformed:     true,
	variant.CodeBlockIDInvalid:      true,
	variant.CodeBlockNested:         true,
	variant.CodeBlockUnclosed:       true,
	variant.CodeBlockUnmatchedClose: true,
	variant.CodeBlockCloseMismatch:  true,
	variant.CodeBlockDuplicateID:    true,
	variant.CodeTargetListEmpty:     true,
	variant.CodeOverrideUnknown:     true,
	variant.CodeTermUnknown:         true,
	variant.CodeTermNameInvalid:     true,
	variant.CodeTermDefaultRequired: true,
}

// IsRenderBlocking reports whether an error-severity validation code must
// stop a render instead of only being reported.
func IsRenderBlocking(code string) bool { return renderBlockingCodes[code] }

// variantIssues converts variant problems into validation issues for one
// path. lineOffset shifts a problem's line to its real position in the file,
// which is non-zero for SKILL.md, whose body starts below the frontmatter.
func variantIssues(path string, lineOffset int, problems []variant.Problem) []Issue {
	issues := make([]Issue, 0, len(problems))
	for _, problem := range problems {
		message := problem.Message
		if problem.Line > 0 {
			message = fmt.Sprintf("line %d: %s", problem.Line+lineOffset, problem.Message)
		}
		issues = append(issues, Issue{Code: problem.Code, Severity: problem.Severity, Message: message, Path: path})
	}
	return issues
}

// validateVariants checks the harness-variant constructs across SKILL.md and
// every markdown reference: marker structure, bundle-wide unique block ids,
// resolvable terms, and overlay overrides that address a block the canonical
// source actually defines. The last check is what keeps an overlay a delta
// keyed to the source instead of a second, drifting copy of it.
func validateVariants(bundle *Bundle) []Issue {
	var issues []Issue
	known := knownTargets()

	paths := make([]string, 0, len(bundle.Markdown)+1)
	for path := range bundle.Markdown {
		paths = append(paths, path)
	}
	slices.Sort(paths)
	paths = append([]string{"SKILL.md"}, paths...)

	// Coupling is only meaningful for a skill that targets more than one
	// harness. A skill deliberately scoped to a single enabled target may
	// name it freely.
	checkCoupling := len(known) > 0 && enabledTargetCount(bundle) != 1

	var sourceIDs []string
	definedIn := map[string]string{}
	for _, path := range paths {
		text := bundle.Body
		lineOffset := bundle.BodyLineOffset
		if path != "SKILL.md" {
			text = bundle.Markdown[path]
			lineOffset = 0
		}
		if checkCoupling {
			for _, mention := range variant.FindMentions(text, known) {
				issues = append(issues, Issue{
					Code:     variant.CodeHarnessCoupling,
					Severity: variant.SeverityWarning,
					Message: fmt.Sprintf(
						"line %d: names harness %q outside any symskills:only region; every other target renders this text too — scope it with a region, move the differing part into a {{term:...}}, or disable the other targets in symskills.toml",
						mention.Line+lineOffset, mention.Name),
					Path: path,
				})
			}
		}
		scan, problems := variant.ScanText(text)
		issues = append(issues, variantIssues(path, lineOffset, problems)...)
		issues = append(issues, variantIssues(path, lineOffset, variant.CheckRegionTargets(scan.Regions, known))...)
		for _, id := range scan.BlockIDs {
			if prev, dup := definedIn[id]; dup {
				issues = append(issues, Issue{
					Code:     variant.CodeBlockDuplicateID,
					Severity: variant.SeverityError,
					Message:  fmt.Sprintf("block id %q is already defined in %s; ids address a block across the whole skill and must be unique", id, prev),
					Path:     path,
				})
				continue
			}
			definedIn[id] = path
			sourceIDs = append(sourceIDs, id)
		}
		for _, name := range scan.Terms {
			if _, ok := bundle.Manifest.Terms[name]; !ok {
				issues = append(issues, Issue{
					Code:     variant.CodeTermUnknown,
					Severity: variant.SeverityError,
					Message:  fmt.Sprintf("term %q is referenced but not defined in [terms]", name),
					Path:     path,
				})
			}
		}
	}

	overrideIDs := map[string][]string{}
	dirs := make([]string, 0, len(bundle.BlockOverrides))
	for dir := range bundle.BlockOverrides {
		dirs = append(dirs, dir)
	}
	slices.Sort(dirs)
	for _, dir := range dirs {
		for id := range bundle.BlockOverrides[dir] {
			overrideIDs[dir] = append(overrideIDs[dir], id)
		}
		if cfg, ok := bundle.Manifest.Targets[dir]; ok && !cfg.Enabled {
			issues = append(issues, Issue{
				Code:     variant.CodeOverrideUnused,
				Severity: variant.SeverityWarning,
				Message:  fmt.Sprintf("target %q is disabled but ships block overrides; they are never rendered", dir),
				Path:     "overlays/" + dir + "/" + variant.BlocksDir,
			})
		}
	}
	issues = append(issues, variantIssues("overlays", 0, variant.CheckOverrides(sourceIDs, overrideIDs))...)
	issues = append(issues, variantIssues("symskills.toml", 0, variant.CheckTerms(bundle.Manifest.Terms, known))...)
	return issues
}

// enabledTargetCount counts the targets a manifest explicitly enables. A
// skill with no manifest returns 0, which renders for every target.
func enabledTargetCount(bundle *Bundle) int {
	count := 0
	for _, cfg := range bundle.Manifest.Targets {
		if cfg.Enabled {
			count++
		}
	}
	return count
}

func safeRelativeFile(root, rel string) error {
	if filepath.IsAbs(rel) {
		return fmt.Errorf("overlay reference %q must be relative", rel)
	}
	clean := filepath.Clean(rel)
	if strings.HasPrefix(clean, ".."+string(filepath.Separator)) || clean == ".." {
		return fmt.Errorf("overlay reference %q escapes skill root", rel)
	}
	if _, err := os.Stat(filepath.Join(root, clean)); err != nil {
		return fmt.Errorf("overlay reference %q: %w", rel, err)
	}
	return nil
}

// HasIssue reports whether issues contains code. It is public for tests and CLI checks.
func HasIssue(issues []Issue, code string) bool {
	return slices.ContainsFunc(issues, func(issue Issue) bool {
		return issue.Code == code
	})
}

// ImportSkill copies an existing skill directory into a managed library.
func ImportSkill(srcRoot, libraryDir string) (ImportResult, error) {
	return ImportSkillOpts(srcRoot, libraryDir, ImportOptions{VCSEnabled: true})
}

// ImportOptions controls how ImportSkillOpts performs an import.
type ImportOptions struct {
	// VCSEnabled enables per-skill git versioning (#118): after a
	// successful copy the skill gets its own repository with an initial
	// commit, and updates produce an automatic commit. Defaults to true.
	VCSEnabled bool
	// Update re-imports over an existing library skill instead of
	// refusing, replacing the tracked tree and recording a new commit.
	Update bool
}

// ImportSkillOpts copies an existing skill directory into a managed
// library, honoring ImportOptions. With Update set, an existing library
// entry is replaced in place (its .git repository survives the swap) and
// the change is committed; without it, a pre-existing entry is an error.
// When versioning is enabled and git is available, a fresh import ends
// with exactly one commit containing the full skill. Versioning failures
// after a successful copy degrade to a VCSWarning, never an import error.
func ImportSkillOpts(srcRoot, libraryDir string, opts ImportOptions) (ImportResult, error) {
	bundle, err := LoadBundle(srcRoot)
	if err != nil {
		return ImportResult{}, err
	}
	name := bundle.Frontmatter.Name
	if name == "" {
		return ImportResult{}, fmt.Errorf("cannot import skill without name")
	}
	dst := filepath.Join(libraryDir, name)
	exists := false
	if _, err := os.Stat(dst); err == nil {
		exists = true
	} else if !errors.Is(err, os.ErrNotExist) {
		return ImportResult{}, err
	}
	if exists && !opts.Update {
		return ImportResult{}, fmt.Errorf("skill %q already exists in library", name)
	}
	operation := "import"
	if exists {
		operation = "update"
		if err := replaceTreeContents(srcRoot, dst); err != nil {
			return ImportResult{}, err
		}
	} else {
		if err := fsutil.CopyTree(srcRoot, dst, func(rel string, d os.DirEntry) bool {
			return d.Name() == ".git" && d.IsDir()
		}); err != nil {
			return ImportResult{}, err
		}
	}
	result := ImportResult{Name: name, Path: dst}
	if !opts.VCSEnabled {
		return result, nil
	}
	// Versioning is best-effort: the copy already succeeded, so git
	// problems degrade to a warning instead of failing the import. A
	// missing git binary is reported by the CLI once per command, so it
	// stays silent here (ErrUnavailable) to avoid per-skill noise.
	if _, err := vcs.Init(dst); err != nil {
		if !errors.Is(err, vcs.ErrUnavailable) {
			result.VCSWarning = fmt.Sprintf("versioning unavailable for %s: %v", name, err)
		}
		return result, nil
	}
	if _, err := vcs.Commit(dst, fmt.Sprintf("%s: skill %s from %s", operation, name, srcRoot)); err != nil {
		if !errors.Is(err, vcs.ErrUnavailable) {
			result.VCSWarning = fmt.Sprintf("versioning degraded for %s: %v", name, err)
		}
	}
	return result, nil
}

// replaceTreeContents replaces every file of dst with a fresh copy of src,
// preserving dst/.git so versioning survives an update. The copy happens
// in a temporary sibling directory first so a failed copy never leaves
// dst half-written.
func replaceTreeContents(src, dst string) error {
	tmp, err := os.MkdirTemp(filepath.Dir(dst), ".import-tmp-*")
	if err != nil {
		return err
	}
	defer os.RemoveAll(tmp)
	if err := fsutil.CopyTree(src, tmp, func(rel string, d os.DirEntry) bool {
		return d.Name() == ".git" && d.IsDir()
	}); err != nil {
		return err
	}
	entries, err := os.ReadDir(dst)
	if err != nil {
		return err
	}
	for _, entry := range entries {
		if entry.Name() == ".git" {
			continue
		}
		if err := os.RemoveAll(filepath.Join(dst, entry.Name())); err != nil {
			return err
		}
	}
	tmpEntries, err := os.ReadDir(tmp)
	if err != nil {
		return err
	}
	for _, entry := range tmpEntries {
		if err := os.Rename(filepath.Join(tmp, entry.Name()), filepath.Join(dst, entry.Name())); err != nil {
			return err
		}
	}
	return nil
}

// isSkillDir reports whether path contains a SKILL.md file.
func isSkillDir(path string) bool {
	fi, err := os.Stat(filepath.Join(path, "SKILL.md"))
	return err == nil && fi.Mode().IsRegular()
}

// ImportSkills discovers and imports skill directories from a parent directory.
// If the given directory itself contains SKILL.md it falls back to a single-skill import.
// Otherwise it scans immediate subdirectories for SKILL.md and imports each one.
// It never fails entirely on a single bad subdirectory — per-item results are returned.
func ImportSkills(parentDir, libraryDir string) []BatchImportResult {
	return ImportSkillsOpts(parentDir, libraryDir, ImportOptions{VCSEnabled: true})
}

// ImportSkillsOpts is ImportSkills with ImportOptions (versioning toggle
// and update semantics) applied to every imported skill.
func ImportSkillsOpts(parentDir, libraryDir string, opts ImportOptions) []BatchImportResult {
	// If the directory itself is a skill, fall back to single import.
	if isSkillDir(parentDir) {
		res, err := ImportSkillOpts(parentDir, libraryDir, opts)
		if err != nil {
			return []BatchImportResult{{
				Name:   filepath.Base(parentDir),
				Status: BatchFailed,
				Error:  err.Error(),
			}}
		}
		return []BatchImportResult{{
			Name:       res.Name,
			Path:       res.Path,
			Status:     BatchImported,
			VCSWarning: res.VCSWarning,
		}}
	}

	entries, err := os.ReadDir(parentDir)
	if err != nil {
		return []BatchImportResult{{
			Name:   filepath.Base(parentDir),
			Status: BatchFailed,
			Error:  err.Error(),
		}}
	}

	var results []BatchImportResult
	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		subdir := filepath.Join(parentDir, entry.Name())
		if !isSkillDir(subdir) {
			continue
		}
		res, err := ImportSkillOpts(subdir, libraryDir, opts)
		if err != nil {
			results = append(results, BatchImportResult{
				Name:   entry.Name(),
				Status: BatchFailed,
				Error:  err.Error(),
			})
			continue
		}
		if res.Name != "" && res.Path != "" {
			results = append(results, BatchImportResult{
				Name:       res.Name,
				Path:       res.Path,
				Status:     BatchImported,
				VCSWarning: res.VCSWarning,
			})
		}
	}
	return results
}

// ListLibrary returns loaded bundles under a library directory.
func ListLibrary(libraryDir string) ([]*Bundle, []Issue) {
	entries, err := os.ReadDir(libraryDir)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil, nil
		}
		return nil, []Issue{{Code: "library_read", Severity: "error", Message: err.Error(), Path: libraryDir}}
	}
	var bundles []*Bundle
	var issues []Issue
	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		root := filepath.Join(libraryDir, entry.Name())
		fm, err := loadFrontmatterOnly(root)
		if err != nil {
			issues = append(issues, Issue{Code: "skill_load", Severity: "error", Message: err.Error(), Path: entry.Name()})
			continue
		}
		abs, err := filepath.Abs(root)
		if err != nil {
			issues = append(issues, Issue{Code: "skill_load", Severity: "error", Message: err.Error(), Path: entry.Name()})
			continue
		}
		bundles = append(bundles, &Bundle{Root: abs, Frontmatter: fm, Manifest: Manifest{Targets: map[string]TargetConfig{}}})
	}
	NormalizeCategories(bundles)
	return bundles, issues
}
