package profile

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

// ResolvedSkill is one skill link after inheritance and context merging.
type ResolvedSkill struct {
	Name    string `json:"name"`  // link name in the profile
	Skill   string `json:"skill"` // skill name in the managed library
	Alias   string `json:"alias,omitempty"`
	Source  string `json:"source"`  // context that provided the link (e.g. "global", "parent:1", "project")
	Profile string `json:"profile"` // profile name that provided the link
}

// sourceContext describes one profile search location, ordered from most
// specific (project) to least specific (global).
type sourceContext struct {
	name string
	dir  string
}

// Resolve loads profileName across all configured contexts and merges links
// with deterministic precedence: global -> parent -> project. Later contexts
// override earlier contexts by link name. Inheritance cycles are rejected.
func Resolve(libraryDir, globalProfilesDir, projectDir, profileName string) ([]ResolvedSkill, []skill.Issue, error) {
	if err := skill.ValidateSkillName(profileName); err != nil {
		return nil, nil, fmt.Errorf("invalid profile name: %w", err)
	}

	ctxs := buildContexts(globalProfilesDir, projectDir)
	if len(ctxs) == 0 {
		return nil, nil, fmt.Errorf("no profile contexts configured")
	}

	resolvedByContext := make(map[string][]ResolvedSkill)
	for _, ctx := range ctxs {
		path := filepath.Join(ctx.dir, profileName+".toml")
		if _, err := os.Stat(path); err != nil {
			if errors.Is(err, os.ErrNotExist) {
				continue
			}
			return nil, nil, fmt.Errorf("stat profile %s: %w", path, err)
		}
		resolved, err := resolveAtContext(ctxs, ctx, profileName, map[string]bool{})
		if err != nil {
			return nil, nil, err
		}
		resolvedByContext[ctx.name] = resolved
	}

	// Merge in precedence order: global first, then parents (farthest to
	// closest), then project last. Because ctxs is ordered most-specific to
	// least-specific, iterate in reverse.
	merged := map[string]ResolvedSkill{}
	for i := len(ctxs) - 1; i >= 0; i-- {
		if resolved, ok := resolvedByContext[ctxs[i].name]; ok {
			for _, rs := range resolved {
				merged[rs.Name] = rs
			}
		}
	}

	var result []ResolvedSkill
	for _, rs := range merged {
		result = append(result, rs)
	}
	sort.Slice(result, func(i, j int) bool { return result[i].Name < result[j].Name })

	issues := validateLibrary(libraryDir, profileName, result)
	return result, issues, nil
}

// buildContexts returns profile contexts from most specific to least specific.
func buildContexts(globalProfilesDir, projectDir string) []sourceContext {
	var ctxs []sourceContext
	if projectDir != "" {
		abs, err := filepath.Abs(projectDir)
		if err == nil {
			ctxs = append(ctxs, sourceContext{name: "project", dir: filepath.Join(abs, ".symskills", "profiles")})
			for i, parent := range collectParents(abs) {
				ctxs = append(ctxs, sourceContext{name: fmt.Sprintf("parent:%d", i+1), dir: filepath.Join(parent, ".symskills", "profiles")})
			}
		}
	}
	if globalProfilesDir != "" {
		ctxs = append(ctxs, sourceContext{name: "global", dir: globalProfilesDir})
	}
	return ctxs
}

// collectParents returns parent directories from closest to farthest.
func collectParents(dir string) []string {
	var parents []string
	for {
		parent := filepath.Dir(dir)
		if parent == dir {
			break
		}
		parents = append(parents, parent)
		dir = parent
	}
	return parents
}

// resolveAtContext resolves a profile at a specific context, including its
// inherited profiles. Inherited profiles are searched starting from ctx and
// moving toward less-specific contexts.
func resolveAtContext(ctxs []sourceContext, ctx sourceContext, profileName string, visited map[string]bool) ([]ResolvedSkill, error) {
	if visited[profileName] {
		return nil, fmt.Errorf("profile inheritance cycle detected at %q", profileName)
	}
	visited[profileName] = true
	defer delete(visited, profileName)

	path := filepath.Join(ctx.dir, profileName+".toml")
	p, err := Load(path)
	if err != nil {
		return nil, err
	}

	inherited := map[string]ResolvedSkill{}
	for _, parentName := range p.Inherits {
		if strings.TrimSpace(parentName) == "" {
			return nil, fmt.Errorf("profile %q contains an empty inherits entry", p.Name)
		}
		if err := skill.ValidateSkillName(parentName); err != nil {
			return nil, fmt.Errorf("profile %q inherits invalid name %q: %w", p.Name, parentName, err)
		}
		found := false
		for _, parentCtx := range contextsFrom(ctxs, ctx) {
			parentPath := filepath.Join(parentCtx.dir, parentName+".toml")
			if _, err := os.Stat(parentPath); err == nil {
				resolved, err := resolveAtContext(ctxs, parentCtx, parentName, visited)
				if err != nil {
					return nil, err
				}
				for _, rs := range resolved {
					inherited[rs.Name] = rs
				}
				found = true
				break
			} else if !errors.Is(err, os.ErrNotExist) {
				return nil, fmt.Errorf("stat inherited profile %s: %w", parentName, err)
			}
		}
		if !found {
			return nil, fmt.Errorf("profile %q inherits %q which was not found", p.Name, parentName)
		}
	}

	for linkName, link := range p.Links {
		inherited[linkName] = ResolvedSkill{
			Name:    linkName,
			Skill:   link.Skill,
			Alias:   link.Alias,
			Source:  ctx.name,
			Profile: p.Name,
		}
	}

	var result []ResolvedSkill
	for _, rs := range inherited {
		result = append(result, rs)
	}
	sort.Slice(result, func(i, j int) bool { return result[i].Name < result[j].Name })
	return result, nil
}

// contextsFrom returns ctx and all less-specific contexts (toward global).
func contextsFrom(ctxs []sourceContext, ctx sourceContext) []sourceContext {
	var result []sourceContext
	found := false
	for _, c := range ctxs {
		if c.name == ctx.name {
			found = true
		}
		if found {
			result = append(result, c)
		}
	}
	return result
}

// validateLibrary checks that every resolved skill exists in the library.
func validateLibrary(libraryDir, profileName string, resolved []ResolvedSkill) []skill.Issue {
	var issues []skill.Issue
	for _, rs := range resolved {
		if rs.Skill == "" {
			issues = append(issues, skill.Issue{Code: "profile_missing_skill", Severity: "error", Message: "link has no skill", Path: rs.Name})
			continue
		}
		if _, err := os.Stat(filepath.Join(libraryDir, rs.Skill)); err != nil {
			if errors.Is(err, os.ErrNotExist) {
				issues = append(issues, skill.Issue{
					Code:     "profile_missing_skill",
					Severity: "error",
					Message:  fmt.Sprintf("profile %q links skill %q which is not in the library; import it with: symskills import <path>", profileName, rs.Skill),
					Path:     rs.Name,
				})
			} else {
				issues = append(issues, skill.Issue{Code: "profile_library_read", Severity: "error", Message: err.Error(), Path: rs.Skill})
			}
		}
	}
	return issues
}
