package install_test

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/config"
	"github.com/danieljustus/symaira-brain/internal/skills/install"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

// TestCustomTargetRenderAndInstallEndToEnd verifies that a user-defined
// target declared in config.toml flows through render, install and uninstall
// exactly like a built-in target.
func TestCustomTargetRenderAndInstallEndToEnd(t *testing.T) {
	before := len(render.Targets)
	defer func() { render.Targets = render.Targets[:before] }()

	home := t.TempDir()
	libDir := filepath.Join(home, ".local", "share", "symskills", "library")
	renderDir := filepath.Join(home, ".local", "share", "symskills", "rendered")
	customRoot := filepath.Join(home, ".myagent", "skills")

	// Declare the custom target through the config shape, then register it
	// the same way the CLI does (config -> CustomTargetSpec).
	cfg := &config.Config{Targets: []config.CustomTarget{
		{Name: "myagent", SkillRootUser: customRoot},
	}}
	specs := make([]render.CustomTargetSpec, 0, len(cfg.Targets))
	for _, tgt := range cfg.Targets {
		specs = append(specs, render.CustomTargetSpec{
			Name:          tgt.Name,
			DisplayName:   tgt.DisplayName,
			BinaryName:    tgt.BinaryName,
			SkillRootUser: tgt.SkillRootUser,
		})
	}
	if err := render.RegisterCustomTargets(specs); err != nil {
		t.Fatalf("register: %v", err)
	}

	// Create a minimal skill bundle.
	skillRoot := filepath.Join(libDir, "demo")
	if err := os.MkdirAll(skillRoot, 0o755); err != nil {
		t.Fatal(err)
	}
	content := "---\nname: demo\ndescription: Demo skill\n---\n\nBody\n"
	if err := os.WriteFile(filepath.Join(skillRoot, "SKILL.md"), []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
	bundle, err := skill.LoadBundle(skillRoot)
	if err != nil {
		t.Fatalf("load bundle: %v", err)
	}

	// Render into the managed render dir for the custom target.
	rendered, errs := render.RenderAll(bundle, renderDir, []render.Target{"myagent"})
	if len(rendered) != 1 || len(errs) > 0 {
		t.Fatalf("RenderAll: rendered=%d errs=%v", len(rendered), errs)
	}
	expectedRenderPath := filepath.Join(renderDir, "myagent", "demo")
	if rendered[0].Path != expectedRenderPath {
		t.Errorf("render path = %q, want %q", rendered[0].Path, expectedRenderPath)
	}

	// Install into the custom target root (copy mode to avoid symlink into temp).
	opts := install.Options{HomeDir: home, Scope: render.ScopeUser, Mode: install.ModeCopy}
	result, err := install.Install(install.RenderedSkill{
		Target: "myagent",
		Name:   "demo",
		Path:   rendered[0].Path,
	}, opts)
	if err != nil {
		t.Fatalf("install: %v", err)
	}
	if result.Action != "installed" {
		t.Errorf("install action = %q", result.Action)
	}
	installed := filepath.Join(customRoot, "demo")
	if _, err := os.Stat(filepath.Join(installed, "SKILL.md")); err != nil {
		t.Errorf("installed SKILL.md missing: %v", err)
	}
	if _, err := os.Stat(filepath.Join(installed, ".symskills.json")); err != nil {
		t.Errorf("install marker missing: %v", err)
	}
	// Uninstall safety: marker-gated removal must work for the custom target.
	removed, err := install.Uninstall("myagent", "demo", opts)
	if err != nil {
		t.Fatalf("uninstall: %v", err)
	}
	if !removed {
		t.Error("expected uninstall to remove the install")
	}
	if _, err := os.Stat(installed); !os.IsNotExist(err) {
		t.Errorf("install dir still present after uninstall: %v", err)
	}
}

// TestCustomTargetMetadataFile verifies the optional metadata template is
// written into the rendered skill for a custom target.
func TestCustomTargetMetadataFile(t *testing.T) {
	before := len(render.Targets)
	defer func() { render.Targets = render.Targets[:before] }()

	home := t.TempDir()
	libDir := filepath.Join(home, "library")
	renderDir := filepath.Join(home, "rendered")
	customRoot := filepath.Join(home, ".myagent", "skills")
	tmplPath := filepath.Join(home, "meta.yaml")
	if err := os.WriteFile(tmplPath, []byte("key: value\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	if err := render.RegisterCustomTargets([]render.CustomTargetSpec{
		{Name: "myagent", SkillRootUser: customRoot, MetadataFile: "agents/myagent.yaml", MetadataTemplate: tmplPath},
	}); err != nil {
		t.Fatalf("register: %v", err)
	}

	skillRoot := filepath.Join(libDir, "demo")
	if err := os.MkdirAll(skillRoot, 0o755); err != nil {
		t.Fatal(err)
	}
	content := "---\nname: demo\ndescription: Demo skill\n---\n\nBody\n"
	if err := os.WriteFile(filepath.Join(skillRoot, "SKILL.md"), []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
	bundle, err := skill.LoadBundle(skillRoot)
	if err != nil {
		t.Fatalf("load bundle: %v", err)
	}

	rendered, errs := render.RenderAll(bundle, renderDir, []render.Target{"myagent"})
	if len(rendered) != 1 || len(errs) > 0 {
		t.Fatalf("RenderAll: rendered=%d errs=%v", len(rendered), errs)
	}
	metaPath := filepath.Join(renderDir, "myagent", "demo", "agents", "myagent.yaml")
	data, err := os.ReadFile(metaPath)
	if err != nil {
		t.Fatalf("metadata file missing: %v", err)
	}
	if string(data) != "key: value\n" {
		t.Errorf("metadata content = %q", string(data))
	}
}
