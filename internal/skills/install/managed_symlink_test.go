package install

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

func TestInstallRefusesUnmanagedSymlinkTarget(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: unmanaged\ndescription: test\n---\n")

	// Pre-create an unmanaged directory and a symlink pointing at it: the
	// symlink target exists but carries no marker file.
	unmanagedTarget := t.TempDir()
	writeFile(t, filepath.Join(unmanagedTarget, "SKILL.md"), "unmanaged, no marker")
	dest := filepath.Join(home, ".config", "opencode", "skills", "unmanaged")
	if err := os.MkdirAll(filepath.Dir(dest), 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	if err := os.Symlink(unmanagedTarget, dest); err != nil {
		t.Fatalf("Symlink: %v", err)
	}

	_, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "unmanaged",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy})
	if err == nil {
		t.Fatal("expected refusal to overwrite symlink pointing at an unmanaged skill")
	}
}

func TestInstallOverManagedSymlinkTargetSucceeds(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: managed\ndescription: test\n---\n")

	managedTarget := t.TempDir()
	writeFile(t, filepath.Join(managedTarget, "SKILL.md"), "managed")
	writeFile(t, filepath.Join(managedTarget, markerFile), "{}")
	dest := filepath.Join(home, ".config", "opencode", "skills", "managed")
	if err := os.MkdirAll(filepath.Dir(dest), 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	if err := os.Symlink(managedTarget, dest); err != nil {
		t.Fatalf("Symlink: %v", err)
	}

	result, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "managed",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy})
	if err != nil {
		t.Fatalf("Install over managed symlink target: %v", err)
	}
	if result.Action != "installed" {
		t.Fatalf("action: want installed, got %q", result.Action)
	}
}

func TestUninstallRefusesUnmanagedSymlinkTarget(t *testing.T) {
	home := t.TempDir()

	unmanagedTarget := t.TempDir()
	writeFile(t, filepath.Join(unmanagedTarget, "SKILL.md"), "unmanaged, no marker")
	dest := filepath.Join(home, ".config", "opencode", "skills", "unmanaged")
	if err := os.MkdirAll(filepath.Dir(dest), 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	if err := os.Symlink(unmanagedTarget, dest); err != nil {
		t.Fatalf("Symlink: %v", err)
	}

	_, err := Uninstall(render.TargetOpenCode, "unmanaged", Options{HomeDir: home, Scope: render.ScopeUser})
	if err == nil {
		t.Fatal("expected refusal to remove symlink pointing at an unmanaged skill")
	}
	if _, statErr := os.Lstat(dest); statErr != nil {
		t.Fatalf("expected symlink left in place, stat err=%v", statErr)
	}
}

func TestUninstallRefusesUnmanagedDirectory(t *testing.T) {
	home := t.TempDir()
	dest := filepath.Join(home, ".config", "opencode", "skills", "unmanaged")
	writeFile(t, filepath.Join(dest, "SKILL.md"), "unmanaged, no marker")

	_, err := Uninstall(render.TargetOpenCode, "unmanaged", Options{HomeDir: home, Scope: render.ScopeUser})
	if err == nil {
		t.Fatal("expected refusal to remove an unmanaged directory")
	}
	if _, statErr := os.Stat(dest); statErr != nil {
		t.Fatalf("expected directory left in place, stat err=%v", statErr)
	}
}

func TestUninstallPropagatesInstallPathError(t *testing.T) {
	home := t.TempDir()
	_, err := Uninstall(render.Target("bogus"), "name", Options{HomeDir: home, Scope: render.ScopeUser})
	if err == nil {
		t.Fatal("expected error for unknown target")
	}
}

func TestUninstallNotInstalledReportsRemovedFalse(t *testing.T) {
	home := t.TempDir()
	removed, err := Uninstall(render.TargetOpenCode, "never-installed", Options{HomeDir: home, Scope: render.ScopeUser})
	if err != nil {
		t.Fatalf("Uninstall of non-installed skill should not error, got: %v", err)
	}
	if removed {
		t.Fatal("expected removed=false when nothing was installed")
	}
}
