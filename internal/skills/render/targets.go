// Package render creates harness-specific skill folders from portable bundles.
package render

import (
	"fmt"
	"path/filepath"

	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
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
			return filepath.Join(home, ".gemini", "config")
		},
		SkillRoot: func(home, project string, scope Scope) string {
			if scope == ScopeProject && project != "" {
				return filepath.Join(project, ".agents", "skills")
			}
			return filepath.Join(home, ".gemini", "config", "skills")
		},
		Quirks: "Global skills live in ~/.gemini/config/skills — the shared config directory the app and the agy CLI both read, not the per-client ~/.gemini/antigravity-cli state directory; workspace skills share <project>/.agents/skills with Codex/OpenClaw",
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
