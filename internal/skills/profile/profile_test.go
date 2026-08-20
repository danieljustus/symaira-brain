package profile

import (
	"os"
	"path/filepath"
	"testing"
)

func writeFile(t *testing.T, path, content string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
}

func TestLoadValidProfile(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "default.toml"), `name = "default"
description = "Default project profile"
inherits = ["base"]

[links.debug]
skill = "debug"

[links.review]
skill = "code-review"
alias = "review"
`)
	p, err := Load(filepath.Join(root, "default.toml"))
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if p.Name != "default" {
		t.Fatalf("name: want default, got %q", p.Name)
	}
	if len(p.Inherits) != 1 || p.Inherits[0] != "base" {
		t.Fatalf("inherits: want [base], got %v", p.Inherits)
	}
	if len(p.Links) != 2 {
		t.Fatalf("want 2 links, got %d", len(p.Links))
	}
	if p.Links["debug"].Skill != "debug" {
		t.Fatalf("debug link: want debug, got %q", p.Links["debug"].Skill)
	}
	if p.Links["review"].Alias != "review" {
		t.Fatalf("review alias: want review, got %q", p.Links["review"].Alias)
	}
}

func TestLoadEmptyLinks(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "empty.toml"), `name = "empty"
`)
	p, err := Load(filepath.Join(root, "empty.toml"))
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if p.Links == nil {
		t.Fatal("Links should be initialised to an empty map")
	}
	if len(p.Links) != 0 {
		t.Fatalf("want 0 links, got %d", len(p.Links))
	}
}

func TestLoadMissingFile(t *testing.T) {
	_, err := Load(filepath.Join(t.TempDir(), "missing.toml"))
	if err == nil {
		t.Fatal("expected error for missing file")
	}
}

func TestValidateProfile(t *testing.T) {
	p := &Profile{
		Name: "default",
		Links: map[string]Link{
			"debug":  {Skill: "debug"},
			"review": {Skill: "code-review", Alias: "review"},
		},
	}
	issues := Validate(p)
	if len(issues) != 0 {
		t.Fatalf("want no issues, got %v", issues)
	}
}

func TestValidateProfileMissingName(t *testing.T) {
	p := &Profile{Links: map[string]Link{"debug": {Skill: "debug"}}}
	issues := Validate(p)
	if len(issues) != 1 || issues[0].Code != "profile_name_required" {
		t.Fatalf("want profile_name_required issue, got %v", issues)
	}
}

func TestValidateProfileInvalidLinkName(t *testing.T) {
	p := &Profile{
		Name:  "default",
		Links: map[string]Link{"Bad Name": {Skill: "debug"}},
	}
	issues := Validate(p)
	if len(issues) == 0 {
		t.Fatal("expected issues for invalid link name")
	}
	found := false
	for _, issue := range issues {
		if issue.Code == "profile_link_name_format" {
			found = true
			break
		}
	}
	if !found {
		t.Fatalf("want profile_link_name_format issue, got %v", issues)
	}
}

func TestValidateProfileMissingSkill(t *testing.T) {
	p := &Profile{
		Name:  "default",
		Links: map[string]Link{"debug": {Skill: ""}},
	}
	issues := Validate(p)
	if len(issues) == 0 {
		t.Fatal("expected issues for missing skill")
	}
	found := false
	for _, issue := range issues {
		if issue.Code == "profile_link_skill_required" {
			found = true
			break
		}
	}
	if !found {
		t.Fatalf("want profile_link_skill_required issue, got %v", issues)
	}
}

func TestValidateNilProfile(t *testing.T) {
	issues := Validate(nil)
	if len(issues) != 1 || issues[0].Code != "profile_required" {
		t.Fatalf("want profile_required issue, got %v", issues)
	}
}

func TestListProfiles(t *testing.T) {
	tmpDir := t.TempDir()

	globalDir := filepath.Join(tmpDir, "global")
	projectDir := filepath.Join(tmpDir, "project")

	// Create global profile
	globalProfilePath := filepath.Join(globalDir, "global-profile.toml")
	writeFile(t, globalProfilePath, `name = "global-profile"
description = "Global profile description"
`)

	// Create overridden profile in global
	overriddenProfilePathGlobal := filepath.Join(globalDir, "common-profile.toml")
	writeFile(t, overriddenProfilePathGlobal, `name = "common-profile"
description = "Common global description"
`)

	// Create project profile
	projectProfilesDir := filepath.Join(projectDir, ".symskills", "profiles")
	projectProfilePath := filepath.Join(projectProfilesDir, "project-profile.toml")
	writeFile(t, projectProfilePath, `name = "project-profile"
description = "Project profile description"
`)

	// Create overridden profile in project (deduplication check)
	overriddenProfilePathProject := filepath.Join(projectProfilesDir, "common-profile.toml")
	writeFile(t, overriddenProfilePathProject, `name = "common-profile"
description = "Common project description"
`)

	refs, err := List(globalDir, projectDir)
	if err != nil {
		t.Fatalf("List: %v", err)
	}

	// Expected list size should be 3: "common-profile", "project-profile", "global-profile"
	// Deduplication: common-profile from project should win because projectDir has higher precedence
	if len(refs) != 3 {
		t.Fatalf("expected 3 refs, got %d: %+v", len(refs), refs)
	}

	var foundCommon, foundProject, foundGlobal bool
	for _, ref := range refs {
		switch ref.Name {
		case "common-profile":
			foundCommon = true
			if ref.Source != "project" {
				t.Errorf("common-profile source: want project, got %q", ref.Source)
			}
			if ref.Description != "Common project description" {
				t.Errorf("common-profile description: want 'Common project description', got %q", ref.Description)
			}
		case "project-profile":
			foundProject = true
			if ref.Source != "project" {
				t.Errorf("project-profile source: want project, got %q", ref.Source)
			}
		case "global-profile":
			foundGlobal = true
			if ref.Source != "global" {
				t.Errorf("global-profile source: want global, got %q", ref.Source)
			}
		default:
			t.Errorf("unexpected profile name: %q", ref.Name)
		}
	}

	if !foundCommon {
		t.Error("common-profile not found")
	}
	if !foundProject {
		t.Error("project-profile not found")
	}
	if !foundGlobal {
		t.Error("global-profile not found")
	}
}

func TestListProfilesError(t *testing.T) {
	tmpDir := t.TempDir()
	globalDir := filepath.Join(tmpDir, "global")

	// Invalid TOML file
	invalidPath := filepath.Join(globalDir, "invalid.toml")
	writeFile(t, invalidPath, `[invalid`)

	_, err := List(globalDir, "")
	if err == nil {
		t.Fatal("expected error when loading invalid profile")
	}
}
