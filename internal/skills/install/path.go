package install

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

func InstallPath(target render.Target, name string, opts Options) (string, error) {
	if err := skill.ValidateSkillName(name); err != nil {
		return "", fmt.Errorf("invalid install name for target %s: %w", target, err)
	}
	home := opts.HomeDir
	if home == "" {
		var err error
		home, err = os.UserHomeDir()
		if err != nil {
			return "", err
		}
	}
	project := opts.ProjectDir
	if project == "" {
		cwd, err := os.Getwd()
		if err == nil {
			project = cwd
		}
	}
	if opts.Scope == "" {
		opts.Scope = render.ScopeUser
	}
	spec, ok := render.LookupSpec(target)
	if !ok {
		return "", fmt.Errorf("unknown target %s", target)
	}
	root := spec.SkillRoot(home, project, opts.Scope)
	return filepath.Join(root, name), nil
}

// TargetDir returns the base installation directory for a target without requiring a skill name.
func TargetDir(target render.Target, opts Options) (string, error) {
	path, err := InstallPath(target, "placeholder", opts)
	if err != nil {
		return "", err
	}
	return filepath.Dir(path), nil
}

// Uninstall removes a managed installed skill. It reports whether an
// installation was actually removed (removed == false means nothing was
// installed at the resolved path).
