package profile

import (
	"path/filepath"
	"slices"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

func TestResolveSingleProfile(t *testing.T) {
	lib := t.TempDir()
	profiles := t.TempDir()
	project := t.TempDir()
	makeSkill(t, lib, "debug")

	writeFile(t, filepath.Join(profiles, "default.toml"), `name = "default"

[links.debug]
skill = "debug"
`)

	resolved, issues, err := Resolve(lib, profiles, project, "default")
	if err != nil {
		t.Fatalf("Resolve: %v", err)
	}
	if len(issues) != 0 {
		t.Fatalf("unexpected issues: %v", issues)
	}
	if len(resolved) != 1 || resolved[0].Name != "debug" || resolved[0].Skill != "debug" {
		t.Fatalf("want debug link, got %v", resolved)
	}
	if resolved[0].Source != "global" {
		t.Fatalf("source: want global, got %q", resolved[0].Source)
	}
}

func TestResolveInheritance(t *testing.T) {
	lib := t.TempDir()
	profiles := t.TempDir()
	project := t.TempDir()
	makeSkill(t, lib, "debug")
	makeSkill(t, lib, "review")

	writeFile(t, filepath.Join(profiles, "base.toml"), `name = "base"

[links.debug]
skill = "debug"
`)
	writeFile(t, filepath.Join(profiles, "default.toml"), `name = "default"
inherits = ["base"]

[links.review]
skill = "review"
`)

	resolved, issues, err := Resolve(lib, profiles, project, "default")
	if err != nil {
		t.Fatalf("Resolve: %v", err)
	}
	if len(issues) != 0 {
		t.Fatalf("unexpected issues: %v", issues)
	}
	if len(resolved) != 2 {
		t.Fatalf("want 2 resolved skills, got %d", len(resolved))
	}
	byName := map[string]ResolvedSkill{}
	for _, rs := range resolved {
		byName[rs.Name] = rs
	}
	if byName["debug"].Source != "global" || byName["debug"].Profile != "base" {
		t.Fatalf("debug source/profile mismatch: %+v", byName["debug"])
	}
	if byName["review"].Source != "global" || byName["review"].Profile != "default" {
		t.Fatalf("review source/profile mismatch: %+v", byName["review"])
	}
}

func TestResolveContextPrecedence(t *testing.T) {
	lib := t.TempDir()
	profiles := t.TempDir()
	project := t.TempDir()
	makeSkill(t, lib, "debug")
	makeSkill(t, lib, "debug-pro")

	writeFile(t, filepath.Join(profiles, "default.toml"), `name = "default"

[links.debug]
skill = "debug"
`)
	writeFile(t, filepath.Join(project, ".symskills", "profiles", "default.toml"), `name = "default"

[links.debug]
skill = "debug-pro"
`)

	resolved, issues, err := Resolve(lib, profiles, project, "default")
	if err != nil {
		t.Fatalf("Resolve: %v", err)
	}
	if len(issues) != 0 {
		t.Fatalf("unexpected issues: %v", issues)
	}
	if len(resolved) != 1 || resolved[0].Skill != "debug-pro" {
		t.Fatalf("want project override debug-pro, got %v", resolved)
	}
	if resolved[0].Source != "project" {
		t.Fatalf("source: want project, got %q", resolved[0].Source)
	}
}

func TestResolveDeterministicOrder(t *testing.T) {
	lib := t.TempDir()
	profiles := t.TempDir()
	project := t.TempDir()
	makeSkill(t, lib, "a")
	makeSkill(t, lib, "b")
	makeSkill(t, lib, "c")

	writeFile(t, filepath.Join(profiles, "default.toml"), `name = "default"

[links.c]
skill = "c"

[links.a]
skill = "a"

[links.b]
skill = "b"
`)

	resolved, _, err := Resolve(lib, profiles, project, "default")
	if err != nil {
		t.Fatalf("Resolve: %v", err)
	}
	want := []string{"a", "b", "c"}
	got := make([]string, len(resolved))
	for i, rs := range resolved {
		got[i] = rs.Name
	}
	if !slices.Equal(got, want) {
		t.Fatalf("order: want %v, got %v", want, got)
	}
}

func TestResolveCycleDetection(t *testing.T) {
	profiles := t.TempDir()
	project := t.TempDir()

	writeFile(t, filepath.Join(profiles, "a.toml"), `name = "a"
inherits = ["b"]

[links.x]
skill = "x"
`)
	writeFile(t, filepath.Join(profiles, "b.toml"), `name = "b"
inherits = ["a"]

[links.y]
skill = "y"
`)

	_, _, err := Resolve("", profiles, project, "a")
	if err == nil {
		t.Fatal("expected cycle detection error")
	}
	if !skill.HasIssue([]skill.Issue{{Message: err.Error()}}, "") && err.Error() == "" {
		t.Fatal("expected non-empty error message")
	}
}

func TestResolveMissingInherit(t *testing.T) {
	profiles := t.TempDir()
	project := t.TempDir()

	writeFile(t, filepath.Join(profiles, "default.toml"), `name = "default"
inherits = ["missing"]

[links.x]
skill = "x"
`)

	_, _, err := Resolve("", profiles, project, "default")
	if err == nil {
		t.Fatal("expected error for missing inherited profile")
	}
}

func TestResolveMissingSkill(t *testing.T) {
	lib := t.TempDir()
	profiles := t.TempDir()
	project := t.TempDir()

	writeFile(t, filepath.Join(profiles, "default.toml"), `name = "default"

[links.debug]
skill = "debug"
`)

	resolved, issues, err := Resolve(lib, profiles, project, "default")
	if err != nil {
		t.Fatalf("Resolve: %v", err)
	}
	if len(issues) != 1 || issues[0].Code != "profile_missing_skill" {
		t.Fatalf("want profile_missing_skill issue, got %v", issues)
	}
	if len(resolved) != 1 || resolved[0].Skill != "debug" {
		t.Fatalf("want debug link even when missing, got %v", resolved)
	}
}

func TestResolveParentContext(t *testing.T) {
	lib := t.TempDir()
	profiles := t.TempDir()
	root := t.TempDir()
	parent := filepath.Join(root, "parent")
	project := filepath.Join(parent, "project")
	makeSkill(t, lib, "debug")
	makeSkill(t, lib, "review")

	writeFile(t, filepath.Join(parent, ".symskills", "profiles", "default.toml"), `name = "default"

[links.debug]
skill = "debug"
`)
	writeFile(t, filepath.Join(project, ".symskills", "profiles", "default.toml"), `name = "default"

[links.review]
skill = "review"
`)

	resolved, issues, err := Resolve(lib, profiles, project, "default")
	if err != nil {
		t.Fatalf("Resolve: %v", err)
	}
	if len(issues) != 0 {
		t.Fatalf("unexpected issues: %v", issues)
	}
	byName := map[string]ResolvedSkill{}
	for _, rs := range resolved {
		byName[rs.Name] = rs
	}
	if byName["debug"].Source != "parent:1" {
		t.Fatalf("debug source: want parent:1, got %q", byName["debug"].Source)
	}
	if byName["review"].Source != "project" {
		t.Fatalf("review source: want project, got %q", byName["review"].Source)
	}
}

func makeSkill(t *testing.T, lib, name string) {
	t.Helper()
	writeFile(t, filepath.Join(lib, name, "SKILL.md"), "---\nname: "+name+"\ndescription: "+name+"\n---\n")
}
