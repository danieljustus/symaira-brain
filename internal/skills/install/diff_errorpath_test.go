package install

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

func writeTree(t *testing.T, root string, files map[string]string) {
	t.Helper()
	for name, content := range files {
		path := filepath.Join(root, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
}

func TestDiffTwoWayReportsChanges(t *testing.T) {
	rendered := t.TempDir()
	installed := t.TempDir()
	writeTree(t, rendered, map[string]string{"SKILL.md": "v2", "new.txt": "n"})
	writeTree(t, installed, map[string]string{"SKILL.md": "v1", "old.txt": "o"})
	changes, err := Diff(rendered, installed, Options{})
	if err != nil {
		t.Fatal(err)
	}
	byPath := map[string]string{}
	for _, c := range changes {
		byPath[c.Path] = c.Status
	}
	if byPath["SKILL.md"] != "modified" || byPath["new.txt"] != "added" || byPath["old.txt"] != "removed" {
		t.Fatalf("unexpected changes: %+v", byPath)
	}
}

func TestDiffFailsWhenInstalledUnreadable(t *testing.T) {
	rendered := t.TempDir()
	writeTree(t, rendered, map[string]string{"SKILL.md": "v1"})
	// fileHashes treats a missing installed path as an empty tree (by
	// design, so a never-installed skill diffs as all-added); an unreadable
	// installed directory is the error path.
	installed := t.TempDir()
	if err := os.WriteFile(filepath.Join(installed, "x.txt"), []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(installed, 0o000); err != nil {
		t.Fatal(err)
	}
	defer os.Chmod(installed, 0o755)
	if _, err := Diff(rendered, installed, Options{}); err == nil {
		t.Fatal("expected error for unreadable installed path")
	}
}

func TestDiffSymlinkUsesBaseSnapshotWhenPresent(t *testing.T) {
	rendered := t.TempDir()
	writeTree(t, rendered, map[string]string{"SKILL.md": "v2"})
	installed := filepath.Join(t.TempDir(), "installed-link")
	if err := os.Symlink(rendered, installed); err != nil {
		t.Fatal(err)
	}
	base := t.TempDir()
	// Diff derives the base snapshot name from filepath.Base(renderedPath).
	baseDir, err := BasePath(render.TargetOpenCode, filepath.Base(rendered), Options{BaseDir: base, Scope: render.ScopeUser})
	if err != nil {
		t.Fatal(err)
	}
	writeTree(t, baseDir, map[string]string{"manifest.json": "{}", "SKILL.md": "v1"})
	opts := Options{BaseDir: base, Target: render.TargetOpenCode, Scope: render.ScopeUser}
	// With a base snapshot, the identical rendered/installed tree diverges
	// from the base, so the three-way diff reports SKILL.md as modified.
	changes, err := Diff(rendered, installed, opts)
	if err != nil {
		t.Fatal(err)
	}
	found := false
	for _, c := range changes {
		if c.Path == "SKILL.md" && c.Status == "modified" {
			found = true
		}
	}
	if !found {
		t.Fatalf("expected base-anchored modified change, got %+v", changes)
	}
	// Without a base, the two-way diff of identical trees reports nothing.
	changes, err = Diff(rendered, installed, Options{})
	if err != nil {
		t.Fatal(err)
	}
	if len(changes) != 0 {
		t.Fatalf("expected no two-way changes for identical trees, got %+v", changes)
	}
}

func TestDiffAgainstBaseThreeWayMatrix(t *testing.T) {
	rendered := t.TempDir()
	installed := t.TempDir()
	baseDir := t.TempDir()
	writeTree(t, rendered, map[string]string{"SKILL.md": "render-v2", "lib-only.txt": "l", "both.txt": "same", "del.txt": "d"})
	writeTree(t, installed, map[string]string{"SKILL.md": "installed-edit", "lib-only.txt": "l", "harness-only.txt": "h", "both.txt": "same", "kept.txt": "k"})
	writeTree(t, baseDir, map[string]string{"manifest.json": "{}", "SKILL.md": "base-v1", "gone.txt": "g", "both.txt": "same", "kept.txt": "k", "del.txt": "d"})
	changes, err := diffAgainstBase(rendered, installed, baseDir)
	if err != nil {
		t.Fatal(err)
	}
	byPath := map[string]string{}
	for _, c := range changes {
		byPath[c.Path] = c.Status
	}
	expect := map[string]string{
		"SKILL.md":         "modified",
		"lib-only.txt":     "added",
		"harness-only.txt": "added",
		"del.txt":          "removed", // in base + rendered, deleted from harness
		"kept.txt":         "removed", // in base + installed only (kept on harness side)
	}
	for path, want := range expect {
		if byPath[path] != want {
			t.Errorf("%s status = %q, want %q (got %+v)", path, byPath[path], want, byPath)
		}
	}
	if _, ok := byPath["gone.txt"]; ok {
		t.Errorf("baseline-only deletion on both current sides must be omitted: %+v", byPath)
	}
	if _, ok := byPath["both.txt"]; ok {
		t.Errorf("unchanged both.txt must not appear: %+v", byPath)
	}
	if _, ok := byPath["manifest.json"]; ok {
		t.Errorf("manifest.json must be excluded from the base side: %+v", byPath)
	}
}

func TestDiffAgainstBaseFailsWhenBaseUnreadable(t *testing.T) {
	rendered := t.TempDir()
	installed := t.TempDir()
	writeTree(t, rendered, map[string]string{"SKILL.md": "v1"})
	writeTree(t, installed, map[string]string{"SKILL.md": "v1"})
	// fileHashes treats a missing base as an empty snapshot (by design);
	// an unreadable base directory is the error path.
	baseDir := t.TempDir()
	if err := os.WriteFile(filepath.Join(baseDir, "x.txt"), []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(baseDir, 0o000); err != nil {
		t.Fatal(err)
	}
	defer os.Chmod(baseDir, 0o755)
	if _, err := diffAgainstBase(rendered, installed, baseDir); err == nil {
		t.Fatal("expected error for unreadable base dir")
	}
}

func TestDiffFailsWhenRenderedUnreadable(t *testing.T) {
	rendered := t.TempDir()
	installed := t.TempDir()
	writeTree(t, rendered, map[string]string{"SKILL.md": "v1"})
	writeTree(t, installed, map[string]string{"SKILL.md": "v1"})
	if err := os.Chmod(rendered, 0o000); err != nil {
		t.Fatal(err)
	}
	defer os.Chmod(rendered, 0o755)
	if _, err := Diff(rendered, installed, Options{}); err == nil {
		t.Fatal("expected error for unreadable rendered path")
	}
}

func TestDiffAgainstBaseFailsWhenRenderedUnreadable(t *testing.T) {
	rendered := t.TempDir()
	installed := t.TempDir()
	baseDir := t.TempDir()
	writeTree(t, rendered, map[string]string{"SKILL.md": "v1"})
	writeTree(t, installed, map[string]string{"SKILL.md": "v1"})
	writeTree(t, baseDir, map[string]string{"manifest.json": "{}", "SKILL.md": "v1"})
	if err := os.Chmod(rendered, 0o000); err != nil {
		t.Fatal(err)
	}
	defer os.Chmod(rendered, 0o755)
	if _, err := diffAgainstBase(rendered, installed, baseDir); err == nil {
		t.Fatal("expected error for unreadable rendered path")
	}
}

func TestDiffAgainstBaseFailsWhenInstalledUnreadable(t *testing.T) {
	rendered := t.TempDir()
	installed := t.TempDir()
	baseDir := t.TempDir()
	writeTree(t, rendered, map[string]string{"SKILL.md": "v1"})
	writeTree(t, installed, map[string]string{"SKILL.md": "v1"})
	writeTree(t, baseDir, map[string]string{"manifest.json": "{}", "SKILL.md": "v1"})
	if err := os.Chmod(installed, 0o000); err != nil {
		t.Fatal(err)
	}
	defer os.Chmod(installed, 0o755)
	if _, err := diffAgainstBase(rendered, installed, baseDir); err == nil {
		t.Fatal("expected error for unreadable installed path")
	}
}

func TestBackupPathCollisionSuffix(t *testing.T) {
	home := t.TempDir()
	opts := Options{HomeDir: home}
	first, err := backupPath("/some/harness/dir/skillname", opts)
	if err != nil {
		t.Fatal(err)
	}
	// Simulate a collision: create the exact path backupPath would return
	// next, forcing the -1 suffix branch.
	base := first
	if err := os.MkdirAll(filepath.Dir(base), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(base, 0o755); err != nil {
		t.Fatal(err)
	}
	second, err := backupPath("/some/harness/dir/skillname", opts)
	if err != nil {
		t.Fatal(err)
	}
	if second == first {
		t.Fatalf("expected suffixed backup path, got %s", second)
	}
	if !strings.HasSuffix(second, "-1") {
		t.Fatalf("expected -1 suffix, got %s", second)
	}
}

func TestBackupPathHomeLookupFailure(t *testing.T) {
	t.Setenv("HOME", "")
	opts := Options{}
	if _, err := backupPath("/x", opts); err == nil {
		t.Fatal("expected error when home cannot be resolved")
	}
}
