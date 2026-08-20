package install

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

func TestForceAdoptsUnmanagedDirectoryAndKeepsBackup(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: adopted\ndescription: test\n---\n")

	dest := filepath.Join(home, ".config", "opencode", "skills", "adopted")
	writeFile(t, filepath.Join(dest, "SKILL.md"), "hand-written, no marker")

	result, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "adopted",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy, Force: true})
	if err != nil {
		t.Fatalf("forced install: %v", err)
	}
	if result.BackupPath == "" {
		t.Fatal("expected a backup path for the adopted unmanaged skill")
	}
	backed, err := os.ReadFile(filepath.Join(result.BackupPath, "SKILL.md"))
	if err != nil {
		t.Fatalf("reading backup: %v", err)
	}
	if string(backed) != "hand-written, no marker" {
		t.Fatalf("backup content: got %q", string(backed))
	}
	// The backup must live outside every harness skills directory so no agent
	// picks it up as a second skill.
	if within, _ := filepath.Rel(filepath.Dir(dest), result.BackupPath); within != "" && !filepath.IsAbs(within) && within[0] != '.' {
		t.Fatalf("backup %s must not sit inside the harness skills dir", result.BackupPath)
	}
	if _, err := os.Stat(filepath.Join(dest, markerFile)); err != nil {
		t.Fatalf("expected managed marker at dest after adopt: %v", err)
	}
}

func TestForceOverUnmanagedSymlinkLeavesLinkTargetIntact(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: linked\ndescription: test\n---\n")

	unmanagedTarget := t.TempDir()
	writeFile(t, filepath.Join(unmanagedTarget, "SKILL.md"), "unmanaged, no marker")
	dest := filepath.Join(home, ".config", "opencode", "skills", "linked")
	if err := os.MkdirAll(filepath.Dir(dest), 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	if err := os.Symlink(unmanagedTarget, dest); err != nil {
		t.Fatalf("Symlink: %v", err)
	}

	result, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "linked",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy, Force: true})
	if err != nil {
		t.Fatalf("forced install over symlink: %v", err)
	}
	if result.BackupPath != "" {
		t.Fatalf("a symlink holds no content, expected no backup, got %q", result.BackupPath)
	}
	if _, err := os.Stat(filepath.Join(unmanagedTarget, "SKILL.md")); err != nil {
		t.Fatalf("symlink target must be left untouched: %v", err)
	}
}

func TestWithoutForceUnmanagedDestStillRefused(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: guarded\ndescription: test\n---\n")

	dest := filepath.Join(home, ".config", "opencode", "skills", "guarded")
	writeFile(t, filepath.Join(dest, "SKILL.md"), "hand-written, no marker")

	if _, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "guarded",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy}); err == nil {
		t.Fatal("expected refusal without --force")
	}
	data, err := os.ReadFile(filepath.Join(dest, "SKILL.md"))
	if err != nil || string(data) != "hand-written, no marker" {
		t.Fatalf("unmanaged skill must be untouched, got %q err=%v", string(data), err)
	}
}

// Regression: when the harness skills directory is a symlink into the render
// cache, dest resolves to the rendered source itself. Install must not delete
// it to make room for a symlink pointing at what it just removed.
func TestInstallIntoSelfSymlinkedHarnessDirKeepsContent(t *testing.T) {
	home := t.TempDir()
	renderRoot := t.TempDir()
	rendered := filepath.Join(renderRoot, "selfref")
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: selfref\ndescription: test\n---\n")
	// A marker is present, so the unmanaged-skill guard does not fire and the
	// install would proceed to RemoveAll(dest) without the sameLocation check.
	writeFile(t, filepath.Join(rendered, markerFile), "{}")

	// ~/.config/opencode/skills -> renderRoot
	skillsDir := filepath.Join(home, ".config", "opencode", "skills")
	if err := os.MkdirAll(filepath.Dir(skillsDir), 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	if err := os.Symlink(renderRoot, skillsDir); err != nil {
		t.Fatalf("Symlink: %v", err)
	}

	result, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "selfref",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeSymlink})
	if err != nil {
		t.Fatalf("install into self-symlinked harness dir: %v", err)
	}
	data, err := os.ReadFile(filepath.Join(rendered, "SKILL.md"))
	if err != nil {
		t.Fatalf("rendered skill must survive: %v", err)
	}
	if !strings.Contains(string(data), "name: selfref") {
		t.Fatalf("rendered content clobbered: %q", string(data))
	}
	st, err := os.Lstat(rendered)
	if err != nil {
		t.Fatalf("Lstat: %v", err)
	}
	if st.Mode()&os.ModeSymlink != 0 {
		t.Fatal("rendered skill was replaced by a symlink to itself")
	}
	if result.Action != "current" {
		t.Fatalf("action: want current, got %q", result.Action)
	}
}
