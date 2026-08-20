package install

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

// injectPromotionFailure makes the final rename of the atomic swap
// (tmp -> dest) fail while leaving every other rename untouched.
func injectPromotionFailure(t *testing.T) {
	t.Helper()
	orig := osRename
	osRename = func(old, new string) error {
		if strings.Contains(old, ".tmp-") {
			return errors.New("injected promotion failure")
		}
		return orig(old, new)
	}
	t.Cleanup(func() { osRename = orig })
}

func assertNoSwapLeftovers(t *testing.T, dest string) {
	t.Helper()
	leftovers, err := filepath.Glob(dest + ".tmp-*")
	if err != nil {
		t.Fatal(err)
	}
	if len(leftovers) != 0 {
		t.Fatalf("staging leftovers: %v", leftovers)
	}
	if _, err := os.Lstat(dest + ".bak"); !os.IsNotExist(err) {
		t.Fatalf("backup must be cleaned up, stat err=%v", err)
	}
}

func TestInstallAtomicPromotionFailureRestoresPreviousInstall(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: atomic\ndescription: test\n---\nv1")

	result, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "atomic",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy})
	if err != nil {
		t.Fatalf("first install: %v", err)
	}
	writeFile(t, filepath.Join(result.Path, "version.txt"), "v1-content")

	injectPromotionFailure(t)
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: atomic\ndescription: test\n---\nv2")
	if _, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "atomic",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy}); err == nil {
		t.Fatal("expected the injected promotion failure")
	}

	data, err := os.ReadFile(filepath.Join(result.Path, "version.txt"))
	if err != nil || string(data) != "v1-content" {
		t.Fatalf("previous install must be restored, got %q err=%v", string(data), err)
	}
	assertNoSwapLeftovers(t, result.Path)
}

func TestInstallAtomicSymlinkPromotionFailureRestoresPreviousSymlink(t *testing.T) {
	home := t.TempDir()
	renderedV1 := t.TempDir()
	writeFile(t, filepath.Join(renderedV1, "SKILL.md"), "---\nname: atomic-link\ndescription: test\n---\nv1")

	result, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "atomic-link",
		Path:   renderedV1,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeSymlink})
	if err != nil {
		t.Fatalf("first symlink install: %v", err)
	}
	if target, err := os.Readlink(result.Path); err != nil || target != renderedV1 {
		t.Fatalf("dest must be a symlink to the v1 source, got %q err=%v", target, err)
	}

	injectPromotionFailure(t)
	renderedV2 := t.TempDir()
	writeFile(t, filepath.Join(renderedV2, "SKILL.md"), "---\nname: atomic-link\ndescription: test\n---\nv2")
	if _, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "atomic-link",
		Path:   renderedV2,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeSymlink}); err == nil {
		t.Fatal("expected the injected promotion failure")
	}

	target, err := os.Readlink(result.Path)
	if err != nil {
		t.Fatalf("dest must still be a symlink: %v", err)
	}
	if target != renderedV1 {
		t.Fatalf("dest must still point at the v1 source, got %q", target)
	}
	assertNoSwapLeftovers(t, result.Path)
}

func TestInstallAtomicMaterializeFailureLeavesDestUntouched(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: atomic-ro\ndescription: test\n---\nv1")

	result, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "atomic-ro",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy})
	if err != nil {
		t.Fatalf("first install: %v", err)
	}
	writeFile(t, filepath.Join(result.Path, "version.txt"), "v1-content")

	// Make the harness skills directory read-only so staging the new install
	// fails before dest is ever touched.
	parent := filepath.Dir(result.Path)
	if err := os.Chmod(parent, 0o555); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chmod(parent, 0o755) })

	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: atomic-ro\ndescription: test\n---\nv2")
	if _, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "atomic-ro",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy}); err == nil {
		t.Fatal("expected failure staging into a read-only directory")
	}

	data, err := os.ReadFile(filepath.Join(result.Path, "version.txt"))
	if err != nil || string(data) != "v1-content" {
		t.Fatalf("previous install must be untouched, got %q err=%v", string(data), err)
	}
	assertNoSwapLeftovers(t, result.Path)
}
