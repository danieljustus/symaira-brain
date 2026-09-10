// Package skill loads, validates, and imports portable Agent Skill bundles.
package skill

import (
	"os"
	"regexp"
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
	// MaxResourceSize caps a single resource file in bytes.
	MaxResourceSize = 10 << 20 // 10 MiB
	// MaxInputSize caps control documents and markdown resources read into memory.
	MaxInputSize = 16 << 20 // 16 MiB
	// MaxFrontmatterSize caps the YAML header passed to the parser.
	MaxFrontmatterSize = 64 << 10 // 64 KiB
	// MaxResourceEntries caps the number of directory entries inventoried.
	MaxResourceEntries = 4096
	// MaxTotalResourceBytes caps the aggregate size of inventoried resources.
	MaxTotalResourceBytes = 64 << 20 // 64 MiB
	// MaxResourceDepth caps directory traversal depth below the bundle root.
	MaxResourceDepth = 32
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
	Root        string `json:"root"`
	rootCap     *os.Root
	Frontmatter Frontmatter `json:"frontmatter"`
	Manifest    Manifest    `json:"manifest"`
	Body        string      `json:"body"`
	// Resources inventories every non-SKILL.md file in the bundle (relative
	// path, size, mode, executable flag). Symlinked directories inside the
	// bundle are traversed under their logical link path; symlinked files are
	// resolved to their target and targets outside the bundle are rejected.
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
