package install

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

func TestPullStagesPortableBodyAndSupportMode(t *testing.T) {
	home, lib := t.TempDir(), t.TempDir()
	src := filepath.Join(lib, "pullable")
	if err := os.MkdirAll(filepath.Join(src, "overlays", "opencode"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(src, "SKILL.md"), []byte("---\nname: pullable\ndescription: test\n---\n\nportable v1\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(src, "notes.txt"), []byte("v1\n"), 0o751); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(src, "overlays", "opencode", "prepend.md"), []byte("overlay header\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	bundle, err := skill.LoadBundle(src)
	if err != nil {
		t.Fatal(err)
	}
	out := t.TempDir()
	rendered, errs := render.RenderAll(bundle, out, []render.Target{render.TargetOpenCode})
	if len(errs) > 0 {
		t.Fatal(errs[0])
	}
	if _, err := Install(RenderedSkill{Target: render.TargetOpenCode, Name: "pullable", Path: rendered[0].Path}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy, BaseDir: filepath.Join(home, ".base")}); err != nil {
		t.Fatal(err)
	}
	dest, err := InstallPath(render.TargetOpenCode, "pullable", Options{HomeDir: home, Scope: render.ScopeUser})
	if err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(filepath.Join(dest, "SKILL.md"))
	if err != nil {
		t.Fatal(err)
	}
	data = []byte(strings.Replace(string(data), "portable v1", "portable v2", 1))
	if err := os.WriteFile(filepath.Join(dest, "SKILL.md"), data, 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dest, "added.sh"), []byte("#!/bin/sh\n"), 0o751); err != nil {
		t.Fatal(err)
	}
	if err := os.Remove(filepath.Join(dest, "notes.txt")); err != nil {
		t.Fatal(err)
	}
	result, err := Pull(PullOptions{HomeDir: home, Scope: render.ScopeUser, LibraryDir: lib, BaseDir: filepath.Join(home, ".base"), Target: render.TargetOpenCode, Name: "pullable"})
	if err != nil {
		t.Fatal(err)
	}
	if result.Action != "staged" || result.StagePath == "" {
		t.Fatalf("result=%+v", result)
	}
	if strings.Contains(string(readPullFile(t, filepath.Join(src, "SKILL.md"))), "portable v2") {
		t.Fatal("pull changed library before apply")
	}
	if got := string(readPullFile(t, filepath.Join(result.StagePath, "SKILL.md"))); !strings.Contains(got, "portable v2") || strings.Contains(got, "overlay header") {
		t.Fatalf("staged skill=%q", got)
	}
	if info, err := os.Stat(filepath.Join(result.StagePath, "added.sh")); err != nil || info.Mode().Perm() != 0o751 {
		t.Fatalf("staged mode=%v err=%v", info, err)
	}
	if _, err := os.Stat(filepath.Join(result.StagePath, "notes.txt")); !os.IsNotExist(err) {
		t.Fatalf("deleted resource remains: %v", err)
	}
}

func TestPullRefusesOverlayBodyEdit(t *testing.T) {
	home, lib := t.TempDir(), t.TempDir()
	src := filepath.Join(lib, "refuse")
	if err := os.MkdirAll(filepath.Join(src, "overlays", "opencode"), 0o755); err != nil {
		t.Fatal(err)
	}
	os.WriteFile(filepath.Join(src, "SKILL.md"), []byte("---\nname: refuse\ndescription: test\n---\n\nportable\n"), 0o644)
	os.WriteFile(filepath.Join(src, "overlays", "opencode", "prepend.md"), []byte("overlay header\n"), 0o644)
	bundle, _ := skill.LoadBundle(src)
	out := t.TempDir()
	rendered, errs := render.RenderAll(bundle, out, []render.Target{render.TargetOpenCode})
	if len(errs) > 0 {
		t.Fatal(errs[0])
	}
	if _, err := Install(RenderedSkill{Target: render.TargetOpenCode, Name: "refuse", Path: rendered[0].Path}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy, BaseDir: filepath.Join(home, ".base")}); err != nil {
		t.Fatal(err)
	}
	dest, _ := InstallPath(render.TargetOpenCode, "refuse", Options{HomeDir: home, Scope: render.ScopeUser})
	data := strings.Replace(string(readPullFile(t, filepath.Join(dest, "SKILL.md"))), "overlay header", "changed header", 1)
	os.WriteFile(filepath.Join(dest, "SKILL.md"), []byte(data), 0o644)
	result, err := Pull(PullOptions{HomeDir: home, Scope: render.ScopeUser, LibraryDir: lib, BaseDir: filepath.Join(home, ".base"), Target: render.TargetOpenCode, Name: "refuse"})
	if err == nil || len(result.Refusals) == 0 || !strings.Contains(strings.Join(result.Refusals, " "), "prepend") {
		t.Fatalf("expected overlay refusal result=%+v err=%v", result, err)
	}
	if strings.Contains(string(readPullFile(t, filepath.Join(src, "SKILL.md"))), "changed header") {
		t.Fatal("library changed on refusal")
	}
}

func readPullFile(t *testing.T, path string) []byte {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return data
}
