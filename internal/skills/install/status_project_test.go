package install

import (
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

// writeLibrarySkill creates a library skill bundle with a support file.
func installFixtureCustomBase(t *testing.T, home, lib, name string, targets []render.Target, mode Mode, baseDir string) {
	t.Helper()
	bundle, err := skill.LoadBundle(filepath.Join(lib, name))
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}
	out := t.TempDir()
	rendered, errs := render.RenderAll(bundle, out, targets)
	if len(errs) > 0 {
		t.Fatalf("render: %v", errs[0])
	}
	for _, item := range rendered {
		if _, err := Install(RenderedSkill{Target: item.Target, Name: item.Name, Path: item.Path}, Options{
			HomeDir:         home,
			Scope:           render.ScopeUser,
			Mode:            mode,
			BaseDir:         baseDir,
			AllowExecutable: false,
		}); err != nil {
			t.Fatalf("install %s/%s: %v", item.Target, item.Name, err)
		}
	}
}
