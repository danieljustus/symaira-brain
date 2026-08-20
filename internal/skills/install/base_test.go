package install

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

func defaultBaseDir(home string) string {
	return filepath.Join(home, ".local", "share", "symskills", "base")
}

func TestInstallWritesBaseSnapshot(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: snap\ndescription: test\n---\n")
	writeFile(t, filepath.Join(rendered, "notes.md"), "notes")

	result, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "snap",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeSymlink})
	if err != nil {
		t.Fatalf("Install: %v", err)
	}

	baseDir := filepath.Join(defaultBaseDir(home), "opencode", "snap")
	for _, f := range []string{"SKILL.md", "notes.md"} {
		if _, err := os.Stat(filepath.Join(baseDir, f)); err != nil {
			t.Fatalf("base file %s missing: %v", f, err)
		}
	}
	// The marker is install bookkeeping and must not enter the snapshot.
	if _, err := os.Stat(filepath.Join(baseDir, markerFile)); !os.IsNotExist(err) {
		t.Fatalf("marker must not be snapshotted, stat err=%v", err)
	}

	data, err := os.ReadFile(filepath.Join(baseDir, manifestFile))
	if err != nil {
		t.Fatalf("manifest missing: %v", err)
	}
	var m BaseManifest
	if err := json.Unmarshal(data, &m); err != nil {
		t.Fatalf("parse manifest: %v", err)
	}
	if m.SchemaVersion != BaseSchemaVersion {
		t.Fatalf("schema version: want %d, got %d", BaseSchemaVersion, m.SchemaVersion)
	}
	if m.Target != "opencode" || m.Name != "snap" {
		t.Fatalf("manifest identity: %+v", m)
	}
	if len(m.Files) != 2 {
		t.Fatalf("manifest files: want 2 entries, got %d: %v", len(m.Files), m.Files)
	}
	installedMD, err := os.ReadFile(filepath.Join(result.Path, "SKILL.md"))
	if err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256(installedMD)
	if got := m.Files["SKILL.md"].SHA256; got != hex.EncodeToString(sum[:]) {
		t.Fatalf("manifest SKILL.md sha256 = %q, want %q", got, hex.EncodeToString(sum[:]))
	}
	if got := m.Files["SKILL.md"].Mode; got != "0644" {
		t.Fatalf("manifest SKILL.md mode = %q, want 0644", got)
	}
}

func TestBaseSnapshotRoundTripsExecutableBits(t *testing.T) {
	for _, tt := range []struct {
		name            string
		allowExecutable bool
		wantMode        string
	}{
		{name: "preserved", allowExecutable: true, wantMode: "0755"},
		{name: "stripped", allowExecutable: false, wantMode: "0644"},
	} {
		t.Run(tt.name, func(t *testing.T) {
			home := t.TempDir()
			rendered := t.TempDir()
			writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: exec-base\ndescription: test\n---\n")
			writeFile(t, filepath.Join(rendered, "run.sh"), "#!/bin/sh\necho hi\n")
			if err := os.Chmod(filepath.Join(rendered, "run.sh"), 0o755); err != nil {
				t.Fatal(err)
			}

			result, err := Install(RenderedSkill{
				Target: render.TargetOpenCode,
				Name:   "exec-base",
				Path:   rendered,
			}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeSymlink, AllowExecutable: tt.allowExecutable})
			if err != nil {
				t.Fatalf("Install: %v", err)
			}

			baseDir := filepath.Join(defaultBaseDir(home), "opencode", "exec-base")
			baseFi, err := os.Stat(filepath.Join(baseDir, "run.sh"))
			if err != nil {
				t.Fatalf("base run.sh missing: %v", err)
			}
			if tt.wantMode == "0644" && baseFi.Mode().Perm()&0o111 != 0 {
				t.Fatalf("base run.sh must have exec bit stripped, got %o", baseFi.Mode().Perm())
			}
			if tt.wantMode == "0755" && baseFi.Mode().Perm()&0o111 == 0 {
				t.Fatalf("base run.sh must keep exec bit with AllowExecutable, got %o", baseFi.Mode().Perm())
			}
			// The installed copy (through the symlink) round-trips the same bits.
			installedFi, err := os.Stat(filepath.Join(result.Path, "run.sh"))
			if err != nil {
				t.Fatalf("installed run.sh stat: %v", err)
			}
			if installedFi.Mode().Perm() != baseFi.Mode().Perm() {
				t.Fatalf("installed mode %o != base mode %o", installedFi.Mode().Perm(), baseFi.Mode().Perm())
			}

			data, err := os.ReadFile(filepath.Join(baseDir, manifestFile))
			if err != nil {
				t.Fatal(err)
			}
			var m BaseManifest
			if err := json.Unmarshal(data, &m); err != nil {
				t.Fatal(err)
			}
			if got := m.Files["run.sh"].Mode; got != tt.wantMode {
				t.Fatalf("manifest run.sh mode = %q, want %q", got, tt.wantMode)
			}
		})
	}
}

func TestSubsequentRenderLeavesBaseUntouched(t *testing.T) {
	home := t.TempDir()
	skillDir := t.TempDir()
	writeFile(t, filepath.Join(skillDir, "SKILL.md"), "---\nname: stable\ndescription: test\n---\n")
	writeFile(t, filepath.Join(skillDir, "notes.md"), "notes")

	bundle, err := skill.LoadBundle(skillDir)
	if err != nil {
		t.Fatal(err)
	}
	out := filepath.Join(t.TempDir(), "rendered")
	results, errs := render.RenderAll(bundle, out, []render.Target{render.TargetOpenCode})
	if len(errs) != 0 || len(results) != 1 {
		t.Fatalf("first render: results=%d errs=%v", len(results), errs)
	}
	if _, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   results[0].Name,
		Path:   results[0].Path,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeSymlink}); err != nil {
		t.Fatalf("Install: %v", err)
	}

	baseDir := filepath.Join(defaultBaseDir(home), "opencode", "stable")
	before := hashTree(t, baseDir)

	// A subsequent render of the same skill must leave the base bytes alone.
	if _, errs := render.RenderAll(bundle, out, []render.Target{render.TargetOpenCode}); len(errs) != 0 {
		t.Fatalf("second render errs: %v", errs)
	}
	after := hashTree(t, baseDir)
	if before != after {
		t.Fatalf("base snapshot changed by render:\nbefore: %s\nafter:  %s", before, after)
	}
}

func TestUninstallRemovesBaseAndLeavesTombstone(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: gone\ndescription: test\n---\n")

	if _, err := Install(RenderedSkill{
		Target: render.TargetClaude,
		Name:   "gone",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy}); err != nil {
		t.Fatalf("Install: %v", err)
	}
	baseDir := filepath.Join(defaultBaseDir(home), "claude", "gone")
	if _, err := os.Stat(filepath.Join(baseDir, manifestFile)); err != nil {
		t.Fatalf("base must exist after install: %v", err)
	}

	removed, err := Uninstall(render.TargetClaude, "gone", Options{HomeDir: home, Scope: render.ScopeUser})
	if err != nil {
		t.Fatalf("Uninstall: %v", err)
	}
	if !removed {
		t.Fatal("expected removed=true")
	}
	if _, err := os.Stat(baseDir); !os.IsNotExist(err) {
		t.Fatalf("base must be removed on uninstall, stat err=%v", err)
	}
	if _, err := os.Stat(baseDir + ".tombstone"); err != nil {
		t.Fatalf("tombstone must survive uninstall: %v", err)
	}

	// Reinstalling clears the tombstone and restores the base.
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: gone\ndescription: again\n---\n")
	if _, err := Install(RenderedSkill{
		Target: render.TargetClaude,
		Name:   "gone",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy}); err != nil {
		t.Fatalf("reinstall: %v", err)
	}
	if _, err := os.Stat(baseDir + ".tombstone"); !os.IsNotExist(err) {
		t.Fatalf("tombstone must be cleared on reinstall, stat err=%v", err)
	}
	if _, err := os.Stat(filepath.Join(baseDir, manifestFile)); err != nil {
		t.Fatalf("base must exist after reinstall: %v", err)
	}
}

func TestBaseSnapshotAtomicOnPromotionFailure(t *testing.T) {
	home := t.TempDir()
	rendered := t.TempDir()
	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: atomic-base\ndescription: test\n---\nv1")

	if _, err := Install(RenderedSkill{
		Target: render.TargetOpenCode,
		Name:   "atomic-base",
		Path:   rendered,
	}, Options{HomeDir: home, Scope: render.ScopeUser, Mode: ModeCopy}); err != nil {
		t.Fatalf("Install: %v", err)
	}
	baseDir := filepath.Join(defaultBaseDir(home), "opencode", "atomic-base")
	before := hashTree(t, baseDir)

	writeFile(t, filepath.Join(rendered, "SKILL.md"), "---\nname: atomic-base\ndescription: test\n---\nv2")
	injectPromotionFailure(t)
	if err := WriteBaseSnapshot(rendered, render.TargetOpenCode, "atomic-base", Options{HomeDir: home, Scope: render.ScopeUser}); err == nil {
		t.Fatal("expected the injected promotion failure")
	}

	// The old base must be fully intact: either old or new, never partial.
	after := hashTree(t, baseDir)
	if before != after {
		t.Fatalf("base changed after failed snapshot:\nbefore: %s\nafter:  %s", before, after)
	}
	assertNoSwapLeftovers(t, baseDir)
}

func TestBasePathProjectScopeIsIsolated(t *testing.T) {
	home := t.TempDir()
	user, err := BasePath(render.TargetClaude, "x", Options{HomeDir: home, Scope: render.ScopeUser})
	if err != nil {
		t.Fatal(err)
	}
	proj, err := BasePath(render.TargetClaude, "x", Options{HomeDir: home, Scope: render.ScopeProject})
	if err != nil {
		t.Fatal(err)
	}
	if user == proj {
		t.Fatalf("project-scope base must not collide with user-scope base: %q", user)
	}
	if !strings.HasSuffix(proj, string(filepath.Separator)+"project"+string(filepath.Separator)+"x") {
		t.Fatalf("unexpected project base path %q", proj)
	}
}

// hashTree returns a stable digest of a directory tree (relative path,
// content and permission bits), used to prove byte-identical trees.
func hashTree(t *testing.T, root string) string {
	t.Helper()
	h := sha256.New()
	err := filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(root, path)
		if err != nil {
			return err
		}
		h.Write([]byte(rel))
		h.Write([]byte{0})
		if d.IsDir() {
			return nil
		}
		info, err := d.Info()
		if err != nil {
			return err
		}
		h.Write([]byte{byte(info.Mode().Perm())})
		h.Write([]byte{0})
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		h.Write(data)
		h.Write([]byte{0})
		return nil
	})
	if err != nil {
		t.Fatalf("hashTree(%s): %v", root, err)
	}
	return hex.EncodeToString(h.Sum(nil))
}
