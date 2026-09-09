package install

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

func TestInstallStripsExecutableBitByDefault(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: exec-skill\ndescription: test\n---\n")
	writeFile(t, filepath.Join(rendered, "run.sh"), "#!/bin/sh\necho hi\n")
	if err := os.Chmod(filepath.Join(rendered, "run.sh"), 0o755); err != nil {
		t.Fatal(err)
	}

	result, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "exec-skill",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy})
	if err != nil {
		t.Fatalf("Install: %v", err)
	}

	installed := filepath.Join(result.Path, "run.sh")
	fi, err := os.Stat(installed)
	if err != nil {
		t.Fatalf("installed run.sh missing: %v", err)
	}
	if fi.Mode().Perm()&0o111 != 0 {
		t.Fatalf("executable bit must be stripped by default, got mode %o", fi.Mode().Perm())
	}
	if len(result.ModeChanges) != 1 {
		t.Fatalf("expected 1 mode change, got %#v", result.ModeChanges)
	}
	if result.ModeChanges[0].Path != "run.sh" {
		t.Errorf("mode change path: want run.sh, got %q", result.ModeChanges[0].Path)
	}
	if result.ModeChanges[0].From != "0755" || result.ModeChanges[0].To != "0644" {
		t.Errorf("mode change: want 0755 -> 0644, got %s -> %s", result.ModeChanges[0].From, result.ModeChanges[0].To)
	}
	// The rendered source is stripped as well, so symlink-mode installs
	// expose the same content.
	if fi, err := os.Stat(filepath.Join(rendered, "run.sh")); err != nil || fi.Mode().Perm()&0o111 != 0 {
		t.Fatalf("rendered source should be stripped too, stat err=%v", err)
	}
}

func TestInstallPreservesExecutableBitWithFlag(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: exec-skill\ndescription: test\n---\n")
	writeFile(t, filepath.Join(rendered, "run.sh"), "#!/bin/sh\necho hi\n")
	if err := os.Chmod(filepath.Join(rendered, "run.sh"), 0o755); err != nil {
		t.Fatal(err)
	}

	result, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "exec-skill",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy, AllowExecutable: true})
	if err != nil {
		t.Fatalf("Install: %v", err)
	}

	if len(result.ModeChanges) != 0 {
		t.Fatalf("expected no mode changes with AllowExecutable, got %#v", result.ModeChanges)
	}
	fi, err := os.Stat(filepath.Join(result.Path, "run.sh"))
	if err != nil {
		t.Fatalf("installed run.sh missing: %v", err)
	}
	if fi.Mode().Perm()&0o111 == 0 {
		t.Fatalf("executable bit must be preserved with AllowExecutable, got mode %o", fi.Mode().Perm())
	}
}

func TestInstallSymlinkModeStripsExecutableBit(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: exec-skill\ndescription: test\n---\n")
	writeFile(t, filepath.Join(rendered, "run.sh"), "#!/bin/sh\necho hi\n")
	if err := os.Chmod(filepath.Join(rendered, "run.sh"), 0o755); err != nil {
		t.Fatal(err)
	}

	result, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "exec-skill",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser})
	if err != nil {
		t.Fatalf("Install: %v", err)
	}
	if result.Mode != ModeSymlink {
		t.Fatalf("expected symlink mode install, got %q", result.Mode)
	}
	// The harness sees the rendered tree through the symlink, so the file
	// at the destination must not be executable.
	fi, err := os.Stat(filepath.Join(result.Path, "run.sh"))
	if err != nil {
		t.Fatalf("run.sh through symlink missing: %v", err)
	}
	if fi.Mode().Perm()&0o111 != 0 {
		t.Fatalf("executable bit must be stripped for symlink installs, got mode %o", fi.Mode().Perm())
	}
}

func TestInstallWithoutExecutableResourcesReportsNoModeChanges(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: plain-skill\ndescription: test\n---\n")

	result, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "plain-skill",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy})
	if err != nil {
		t.Fatalf("Install: %v", err)
	}
	if len(result.ModeChanges) != 0 {
		t.Fatalf("expected no mode changes for a bundle without executable resources, got %#v", result.ModeChanges)
	}
}

func TestInstallDryRunReportsPlannedModeChanges(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: exec-skill\ndescription: test\n---\n")
	writeFile(t, filepath.Join(rendered, "run.sh"), "#!/bin/sh\necho hi\n")
	if err := os.Chmod(filepath.Join(rendered, "run.sh"), 0o755); err != nil {
		t.Fatal(err)
	}

	result, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "exec-skill",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy, DryRun: true})
	if err != nil {
		t.Fatalf("Install: %v", err)
	}
	if result.Action != "planned" {
		t.Fatalf("action: want planned, got %q", result.Action)
	}
	if len(result.ModeChanges) != 1 {
		t.Fatalf("expected 1 planned mode change, got %#v", result.ModeChanges)
	}
	// Dry run must not modify the source.
	if fi, err := os.Stat(filepath.Join(rendered, "run.sh")); err != nil || fi.Mode().Perm()&0o111 == 0 {
		t.Fatalf("dry run must not strip the source, stat err=%v", err)
	}
}
