package install

import (
	"os"
	"path/filepath"
	"strings"
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

func TestInstallRefusesUnmanagedCollision(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: collide\ndescription: test\n---\n")
	dest := filepath.Join(home, ".config", "opencode", "skills", "collide")
	writeFile(t, filepath.Join(dest, "SKILL.md"), "unmanaged")

	_, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "collide",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy})
	if err == nil {
		t.Fatal("expected unmanaged collision error")
	}
}

func TestInstallCopyWritesMarkerAndUninstallRemovesManagedSkill(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: managed\ndescription: test\n---\n")

	result, err := Install(RenderedSkill{
		Target: render.TargetClaude,
		Name:   "managed",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy})
	if err != nil {
		t.Fatalf("Install: %v", err)
	}

	if result.Action != "installed" {
		t.Fatalf("action: want installed, got %q", result.Action)
	}
	if _, err := os.Stat(filepath.Join(result.Path, ".symskills.json")); err != nil {
		t.Fatalf("marker missing: %v", err)
	}

	removed, err := Uninstall(render.TargetClaude, "managed", Options{HomeDir: home, Scope: render.ScopeUser})
	if err != nil {
		t.Fatalf("Uninstall: %v", err)
	}
	if !removed {
		t.Fatal("expected Uninstall to report removed=true")
	}
	if _, err := os.Stat(result.Path); !os.IsNotExist(err) {
		t.Fatalf("expected installed skill removed, stat err=%v", err)
	}
}

func TestInstallAndUninstallDanglingSymlink(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: dangling\ndescription: test\n---\n")

	result, err := Install(RenderedSkill{
		Target: render.TargetClaude,
		Name:   "dangling",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeSymlink})
	if err != nil {
		t.Fatalf("Install symlink: %v", err)
	}

	// Remove rendered source to make result.Path a dangling symlink
	if err := os.RemoveAll(rendered); err != nil {
		t.Fatal(err)
	}

	// Verify it is a dangling symlink
	if _, err := os.Stat(result.Path); !os.IsNotExist(err) {
		t.Fatalf("expected stat to fail for dangling symlink")
	}

	// Re-install should succeed over dangling symlink
	newRendered := t.TempDir()
	writeFile(t, filepath.Join(newRendered, "SKILL.md"), "---\nname: dangling\ndescription: test\n---\n")
	_, err = Install(RenderedSkill{
		Target: render.TargetClaude,
		Name:   "dangling",
		Path:   newRendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeSymlink})
	if err != nil {
		t.Fatalf("Re-install over dangling symlink failed: %v", err)
	}

	// Make dangling again for Uninstall test
	if err := os.RemoveAll(newRendered); err != nil {
		t.Fatal(err)
	}
	removed, err := Uninstall(render.TargetClaude, "dangling", Options{HomeDir: home, Scope: render.ScopeUser})
	if err != nil {
		t.Fatalf("Uninstall dangling symlink failed: %v", err)
	}
	if !removed {
		t.Fatal("expected Uninstall to report removed=true for dangling symlink")
	}
	if _, err := os.Lstat(result.Path); !os.IsNotExist(err) {
		t.Fatalf("expected dangling symlink removed")
	}
}

func TestDiffReportsChangedFiles(t *testing.T) {
	rendered := t.TempDir()
	installed := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "new")
	writeFile(t, filepath.Join(installed, "SKILL.md"), "old")

	changes, err := Diff(rendered, installed, Options{})
	if err != nil {
		t.Fatalf("Diff: %v", err)
	}
	if len(changes) != 1 || changes[0].Path != "SKILL.md" || changes[0].Status != "modified" {
		t.Fatalf("unexpected changes: %#v", changes)
	}
}

func TestDiffIncludesContentDiff(t *testing.T) {
	// #110: a modified file must carry a unified-style content diff
	// (left = freshly rendered, right = installed copy) so the CLI/JSON
	// output and the GUI dialog can show what actually changed.
	rendered := t.TempDir()
	installed := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "line one\nline two\nline three\n")
	writeFile(t, filepath.Join(installed, "SKILL.md"), "line one\nline two changed\nline three\n")

	changes, err := Diff(rendered, installed, Options{})
	if err != nil {
		t.Fatalf("Diff: %v", err)
	}
	if len(changes) != 1 || changes[0].Path != "SKILL.md" || changes[0].Status != "modified" {
		t.Fatalf("unexpected changes: %#v", changes)
	}
	diff := changes[0].Diff
	if !strings.Contains(diff, "-line two\n") {
		t.Errorf("expected rendered-only line prefixed with '-', got:\n%s", diff)
	}
	if !strings.Contains(diff, "+line two changed\n") {
		t.Errorf("expected installed-only line prefixed with '+', got:\n%s", diff)
	}
	if !strings.Contains(diff, " ") || !strings.Contains(diff, "line one") {
		t.Errorf("expected unchanged context lines in diff, got:\n%s", diff)
	}
	if !strings.Contains(diff, "@@ ") {
		t.Errorf("expected unified hunk header in diff, got:\n%s", diff)
	}
	if !strings.Contains(diff, "SKILL.md") {
		t.Errorf("expected file header naming the changed path in diff, got:\n%s", diff)
	}
}

func TestDiffSkipsContentForBinaryAndStatusOnlyChanges(t *testing.T) {
	// #110: content diffs are best-effort enrichment. Binary files and
	// added/removed files keep the status-only shape (empty Diff).
	rendered := t.TempDir()
	installed := t.TempDir()
	writeFile(t, filepath.Join(rendered, "asset.bin"), "abc\x00def")
	writeFile(t, filepath.Join(installed, "asset.bin"), "abc\x00ghi")
	writeFile(t, filepath.Join(rendered, "added.txt"), "new file\n")
	writeFile(t, filepath.Join(installed, "removed.txt"), "old file\n")

	changes, err := Diff(rendered, installed, Options{})
	if err != nil {
		t.Fatalf("Diff: %v", err)
	}
	for _, c := range changes {
		if c.Diff != "" {
			t.Errorf("change %s (%s) must not carry a content diff, got:\n%s", c.Path, c.Status, c.Diff)
		}
	}
	if len(changes) != 3 {
		t.Fatalf("expected modified+added+removed changes, got %#v", changes)
	}
}

func TestDiffSymlinkTargetDoesNotCrash(t *testing.T) {
	// Regression test for #64: diff must follow symlinks to the
	// actual render-cache directory (the default symlink install mode).
	tmp := t.TempDir()
	rendered := filepath.Join(tmp, "rendered")
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "new content")
	writeFile(t, filepath.Join(rendered, "notes.md"), "notes")

	// Simulate a symlink-mode install: installed is a symlink to rendered.
	installed := filepath.Join(tmp, "installed-link")
	if err := os.Symlink(rendered, installed); err != nil {
		t.Fatalf("Symlink: %v", err)
	}

	changes, err := Diff(rendered, installed, Options{})
	if err != nil {
		t.Fatalf("Diff through symlink: %v", err)
	}
	if len(changes) != 0 {
		t.Fatalf("expected no changes (identical trees), got %#v", changes)
	}
}

func TestInstallPathRejectsHostileNames(t *testing.T) {
	hostile := []string{"../evil", "evil/name", "/etc/evil", ".."}
	for _, name := range hostile {
		_, err := InstallPath(render.TargetOpenCode, name, Options{})
		if err == nil {
			t.Fatalf("expected error for name %q", name)
		}
	}
}

func TestInstallPathScopeProject(t *testing.T) {
	project := t.TempDir()
	opts := Options{ProjectDir: project, Scope: render.ScopeProject}

	cases := []struct {
		target render.Target
		sub    []string
	}{
		{render.TargetOpenCode, []string{".opencode", "skills", "my-skill"}},
		{render.TargetClaude, []string{".claude", "skills", "my-skill"}},
		{render.TargetCodex, []string{".agents", "skills", "my-skill"}},
		{render.TargetHermes, []string{".hermes", "skills", "my-skill"}},
		{render.TargetAntigravity, []string{".agents", "skills", "my-skill"}},
		{render.TargetOpenClaw, []string{".agents", "skills", "my-skill"}},
	}
	for _, c := range cases {
		got, err := InstallPath(c.target, "my-skill", opts)
		if err != nil {
			t.Fatalf("InstallPath(%s, ScopeProject): %v", c.target, err)
		}
		want := filepath.Join(append([]string{project}, c.sub...)...)
		if got != want {
			t.Errorf("InstallPath(%s, ScopeProject) = %q, want %q", c.target, got, want)
		}
	}

	// unknown target with project scope
	_, err := InstallPath("unknown-target", "my-skill", opts)
	if err == nil {
		t.Fatal("expected error for unknown target with project scope")
	}
}

func TestInstallPathUserAllTargets(t *testing.T) {
	home := t.TempDir()
	opts := Options{HomeDir: home, Scope: render.ScopeUser}

	cases := []struct {
		target render.Target
		sub    []string
	}{
		{render.TargetOpenCode, []string{".config", "opencode", "skills", "my-skill"}},
		{render.TargetClaude, []string{".claude", "skills", "my-skill"}},
		{render.TargetCodex, []string{".agents", "skills", "my-skill"}},
		{render.TargetHermes, []string{".hermes", "skills", "symaira", "my-skill"}},
		{render.TargetAntigravity, []string{".gemini", "config", "skills", "my-skill"}},
		{render.TargetOpenClaw, []string{".openclaw", "skills", "my-skill"}},
	}
	for _, c := range cases {
		got, err := InstallPath(c.target, "my-skill", opts)
		if err != nil {
			t.Fatalf("InstallPath(%s, ScopeUser): %v", c.target, err)
		}
		want := filepath.Join(append([]string{home}, c.sub...)...)
		if got != want {
			t.Errorf("InstallPath(%s, ScopeUser) = %q, want %q", c.target, got, want)
		}
	}

	// unknown target with user scope
	_, err := InstallPath("unknown-target", "my-skill", opts)
	if err == nil {
		t.Fatal("expected error for unknown target with user scope")
	}
}

func TestTargetDir(t *testing.T) {
	home := t.TempDir()
	opts := Options{HomeDir: home, Scope: render.ScopeUser}

	cases := []struct {
		target render.Target
		want   string
	}{
		{render.TargetOpenCode, filepath.Join(home, ".config", "opencode", "skills")},
		{render.TargetClaude, filepath.Join(home, ".claude", "skills")},
		{render.TargetCodex, filepath.Join(home, ".agents", "skills")},
		{render.TargetHermes, filepath.Join(home, ".hermes", "skills", "symaira")},
		{render.TargetAntigravity, filepath.Join(home, ".gemini", "config", "skills")},
		{render.TargetOpenClaw, filepath.Join(home, ".openclaw", "skills")},
	}

	for _, c := range cases {
		got, err := TargetDir(c.target, opts)
		if err != nil {
			t.Fatalf("TargetDir(%s): %v", c.target, err)
		}
		if got != c.want {
			t.Errorf("TargetDir(%s) = %q, want %q", c.target, got, c.want)
		}
	}
}

func writeFile(t *testing.T, path, content string) {
	t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
}
