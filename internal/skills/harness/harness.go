// Package harness provides read-only inventory and readiness status for AI-agent harnesses.
//
// VerificationStatus vs. smoke test
//
// VerificationStatus reports whether the harness install directory is present
// and readable from symskills' perspective — it checks the skill root on disk
// but does NOT confirm the harness runtime can discover and load those skills.
//
// The opt-in headless smoke test (scripts/smoke-harness.sh)  goes one step
// further: it installs a fixture skill into a temp HOME via symskills, then
// drives each available harness binary headless and asserts the skill was
// loaded at runtime.  This exercises the end-to-end path that the harness
// uses to resolve its own skill directory, something that symskills alone
// cannot verify.
//
// Smoke-test results are complementary to the symskills targets command:
//   - targets status `verified`   ↔ symskills confirms skill root on disk
//   - smoke-test `PASS`           ↔ harness runtime loaded the installed skill
//   - smoke-test `SKIP`           ↔ harness binary absent or API key missing
//   - smoke-test `WARN`           ↔ harness ran but skill name not confirmed
//
// The smoke target is wired only into the weekly scheduled CI job; it never
// runs in the PR gate.
package harness

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

type Status struct {
	Target               render.Target `json:"target"`
	DisplayName          string        `json:"display_name"`
	Installed            bool          `json:"installed"`
	Evidence             string        `json:"evidence"`
	EffectiveSkillRoot   string        `json:"effective_skill_root"`
	SkillRootExists      bool          `json:"skill_root_exists"`
	SkillRootReadable    bool          `json:"skill_root_readable"`
	ManagedSkillsCount   int           `json:"managed_skills_count"`
	UnmanagedSkillsCount int           `json:"unmanaged_skills_count"`
	InstallState         string        `json:"install_state"`
	// Capabilities lists what symskills can do *toward* this target.
	Capabilities []string `json:"capabilities"`
	// RuntimeCapabilities reports what the harness runtime offers a skill,
	// as capability name -> "supported" | "unsupported" | "unknown". It is
	// declared data, never probed: unknown means symskills has not been
	// told, not that the harness lacks the capability.
	RuntimeCapabilities map[string]string `json:"runtime_capabilities"`
	SetupHint           string            `json:"setup_hint"`
	VerificationStatus  string            `json:"verification_status"` // "unknown" | "verified" | "not_verified" — see package doc for smoke-test relation
}

type Options struct {
	HomeDir    string       `json:"home_dir,omitempty"`
	ProjectDir string       `json:"project_dir,omitempty"`
	Scope      render.Scope `json:"scope,omitempty"`
}

type Descriptor struct {
	Target      render.Target
	DisplayName string
	BinaryName  string
	ConfigDir   func(home, project string, scope render.Scope) string
	SkillRoot   func(home, project string, scope render.Scope) string
}

// Descriptors returns the list of Descriptors derived from the target
// registry. It is computed on demand so user-defined targets registered at
// runtime are included.
func Descriptors() []Descriptor {
	descs := make([]Descriptor, len(render.Targets))
	for i, spec := range render.Targets {
		descs[i] = Descriptor{
			Target:      spec.Name,
			DisplayName: spec.DisplayName,
			BinaryName:  spec.BinaryName,
			ConfigDir:   spec.ConfigDir,
			SkillRoot:   spec.SkillRoot,
		}
	}
	return descs
}

// ListStatus returns the harness inventory and readiness status for all supported targets.
func ListStatus(opts Options) []Status {
	if opts.HomeDir == "" {
		if h, err := os.UserHomeDir(); err == nil {
			opts.HomeDir = h
		}
	}
	if opts.Scope == "" {
		opts.Scope = render.ScopeUser
	}

	results := make([]Status, 0, len(Descriptors()))
	for _, desc := range Descriptors() {
		results = append(results, inspectTarget(desc, opts))
	}
	return results
}

func inspectTarget(desc Descriptor, opts Options) Status {
	capabilities := []string{"render", "install", "symlink", "copy"}
	skillRoot := desc.SkillRoot(opts.HomeDir, opts.ProjectDir, opts.Scope)
	configDir := desc.ConfigDir(opts.HomeDir, opts.ProjectDir, opts.Scope)

	st := Status{
		Target:              desc.Target,
		DisplayName:         desc.DisplayName,
		EffectiveSkillRoot:  skillRoot,
		Capabilities:        capabilities,
		RuntimeCapabilities: render.CapabilityStates(desc.Target),
		VerificationStatus:  "unknown",
		Evidence:            "none",
	}

	// 1. Evidence / Installed check
	var evidenceParts []string
	if binPath, err := exec.LookPath(desc.BinaryName); err == nil && binPath != "" {
		evidenceParts = append(evidenceParts, fmt.Sprintf("binary:%s", binPath))
		st.Installed = true
		st.VerificationStatus = "verified"
	}
	if info, err := os.Stat(configDir); err == nil && info.IsDir() {
		evidenceParts = append(evidenceParts, fmt.Sprintf("config_dir:%s", configDir))
		st.Installed = true
		if st.VerificationStatus == "unknown" {
			st.VerificationStatus = "verified"
		}
	}
	if len(evidenceParts) > 0 {
		st.Evidence = evidenceParts[0]
		if len(evidenceParts) > 1 {
			st.Evidence = fmt.Sprintf("%s,%s", evidenceParts[0], evidenceParts[1])
		}
	} else {
		st.VerificationStatus = "not_verified"
	}

	// 2. Skill root check
	_, err := os.Lstat(skillRoot)
	if err == nil {
		st.SkillRootExists = true
		// Verify directory readable by reading entries
		entries, readErr := os.ReadDir(skillRoot)
		if readErr == nil {
			st.SkillRootReadable = true
			for _, entry := range entries {
				itemPath := filepath.Join(skillRoot, entry.Name())
				if isManagedSkill(itemPath) {
					st.ManagedSkillsCount++
				} else if entry.IsDir() || (entry.Type()&os.ModeSymlink != 0) {
					st.UnmanagedSkillsCount++
				}
			}
		} else {
			st.SkillRootReadable = false
		}
	} else if os.IsNotExist(err) {
		st.SkillRootExists = false
		st.SkillRootReadable = false
	}

	// 3. Install State & Setup Hint derivation
	if !st.SkillRootExists {
		st.InstallState = "missing"
		st.SetupHint = fmt.Sprintf("Create skill directory %s or run 'symskills install --target %s <skill>'", skillRoot, desc.Target)
	} else if !st.SkillRootReadable {
		st.InstallState = "unreadable"
		st.SetupHint = fmt.Sprintf("Check permissions for skill root %s", skillRoot)
	} else if st.ManagedSkillsCount > 0 && st.UnmanagedSkillsCount == 0 {
		st.InstallState = "managed"
		st.SetupHint = fmt.Sprintf("Harness is active with %d managed skill(s)", st.ManagedSkillsCount)
	} else if st.ManagedSkillsCount > 0 && st.UnmanagedSkillsCount > 0 {
		st.InstallState = "mixed"
		st.SetupHint = fmt.Sprintf("Harness contains %d managed and %d unmanaged skill(s)", st.ManagedSkillsCount, st.UnmanagedSkillsCount)
	} else if st.UnmanagedSkillsCount > 0 {
		st.InstallState = "unmanaged"
		st.SetupHint = fmt.Sprintf("Harness contains %d unmanaged skill(s); consider importing into library", st.UnmanagedSkillsCount)
	} else {
		st.InstallState = "empty"
		st.SetupHint = fmt.Sprintf("Harness skill directory is ready; install skills with 'symskills install --target %s <skill>'", desc.Target)
	}

	return st
}

func isManagedSkill(path string) bool {
	// A skill install directory is managed if it contains .symskills.json or if symlink target contains .symskills.json
	markerPath := filepath.Join(path, ".symskills.json")
	if _, err := os.Stat(markerPath); err == nil {
		return true
	}
	// Check if symlink
	if st, err := os.Lstat(path); err == nil && (st.Mode()&os.ModeSymlink != 0) {
		if target, err := os.Readlink(path); err == nil {
			if !filepath.IsAbs(target) {
				target = filepath.Join(filepath.Dir(path), target)
			}
			if _, err := os.Stat(filepath.Join(target, ".symskills.json")); err == nil {
				return true
			}
		}
	}
	return false
}
