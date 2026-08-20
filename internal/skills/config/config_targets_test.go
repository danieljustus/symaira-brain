package config

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestLoadTargetsFromGlobalConfig(t *testing.T) {
	home := t.TempDir()
	oldHome := os.Getenv("HOME")
	t.Setenv("HOME", home)
	defer os.Setenv("HOME", oldHome)

	cfgDir := filepath.Join(home, ".config", "symskills")
	if err := os.MkdirAll(cfgDir, 0o755); err != nil {
		t.Fatal(err)
	}
	content := `library_dir = "/tmp/lib"

[[targets]]
name = "myagent"
skill_root_user = "/home/u/.myagent/skills"
skill_root_project = "/proj/.myagent/skills"
overlay_dir = "my-overlays"
metadata_file = "agents/myagent.yaml"
metadata_template = "/tmp/meta.yaml"
`
	if err := os.WriteFile(filepath.Join(cfgDir, "config.toml"), []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	targets, err := LoadTargets()
	if err != nil {
		t.Fatalf("LoadTargets: %v", err)
	}
	if len(targets) != 1 {
		t.Fatalf("expected 1 target, got %d", len(targets))
	}
	tt := targets[0]
	if tt.Name != "myagent" || tt.SkillRootUser != "/home/u/.myagent/skills" || tt.SkillRootProject != "/proj/.myagent/skills" {
		t.Errorf("decoded target = %+v", tt)
	}
	if tt.OverlayDir != "my-overlays" || tt.MetadataFile != "agents/myagent.yaml" || tt.MetadataTemplate != "/tmp/meta.yaml" {
		t.Errorf("optional fields not decoded: %+v", tt)
	}
}

func TestLoadTargetsMissingFile(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)

	targets, err := LoadTargets()
	if err != nil {
		t.Fatalf("LoadTargets with no config: %v", err)
	}
	if len(targets) != 0 {
		t.Errorf("expected 0 targets, got %d", len(targets))
	}
}

func TestLoadTargetsProjectOverridesGlobal(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)

	cfgDir := filepath.Join(home, ".config", "symskills")
	if err := os.MkdirAll(cfgDir, 0o755); err != nil {
		t.Fatal(err)
	}
	global := `[[targets]]
name = "myagent"
skill_root_user = "/global/myagent/skills"
`
	if err := os.WriteFile(filepath.Join(cfgDir, "config.toml"), []byte(global), 0o644); err != nil {
		t.Fatal(err)
	}

	cwd, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	project := `[[targets]]
name = "myagent"
skill_root_user = "/project/myagent/skills"
`
	projectPath := filepath.Join(cwd, ".symskills.toml")
	if err := os.WriteFile(projectPath, []byte(project), 0o644); err != nil {
		t.Fatal(err)
	}
	defer os.Remove(projectPath)

	targets, err := LoadTargets()
	if err != nil {
		t.Fatalf("LoadTargets: %v", err)
	}
	if len(targets) != 1 || targets[0].SkillRootUser != "/project/myagent/skills" {
		t.Errorf("project file should override global, got %+v", targets)
	}
}

func TestLoadTargetsRejectsMalformedConfig(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)

	cfgDir := filepath.Join(home, ".config", "symskills")
	if err := os.MkdirAll(cfgDir, 0o755); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(cfgDir, "config.toml")
	if err := os.WriteFile(path, []byte("[[targets]\nname = \"broken\"\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	_, err := LoadTargets()
	if err == nil {
		t.Fatal("expected malformed config to fail")
	}
	if !strings.Contains(err.Error(), path) {
		t.Fatalf("error %q does not identify malformed config path %q", err, path)
	}
}
