package install

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

// editInstalledCopy modifies the installed SKILL.md directly (the harness
// side), leaving the library and the base snapshot untouched. In copy mode
// the installed tree is a real directory, so the edit is unambiguous.
func editInstalledCopy(t *testing.T, path string) {
	t.Helper()
	skillmd := filepath.Join(path, "SKILL.md")
	data, err := os.ReadFile(skillmd)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(skillmd, append(data, []byte("\nedited at the harness\n")...), 0o644); err != nil {
		t.Fatal(err)
	}
}

// TestStatusHarnessEditIsHarnessChanged pins the #126 core: an edit made on
// the installed copy (library unchanged) is reported as harness-changed —
// never as stale — even though the marker's source_hash still matches the
// library (the case the coarse hash comparison cannot see).
func TestStatusHarnessEditIsHarnessChanged(t *testing.T) {
	home := t.TempDir()
	lib := t.TempDir()
	writeLibrarySkill(t, lib, "harnessedit")

	installFixture(t, home, lib, "harnessedit", []render.Target{render.TargetOpenCode}, ModeCopy, false)

	// Sanity: fresh install is in-sync.
	statuses, err := Status(statusOpts(home, lib))
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	if st := findByTarget(t, statuses, render.TargetOpenCode); st.Status != StatusInSync {
		t.Fatalf("fresh install = %s, want in-sync (%+v)", st.Status, st)
	}

	// Edit the installed copy. The library is unchanged, so the marker
	// source hash still matches the fresh hash — but the three-way
	// classification must see the harness-side edit.
	dest, err := InstallPath(render.TargetOpenCode, "harnessedit", Options{HomeDir: home, Scope: render.ScopeUser})
	if err != nil {
		t.Fatal(err)
	}
	editInstalledCopy(t, dest)

	statuses, err = Status(statusOpts(home, lib))
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	st := findByTarget(t, statuses, render.TargetOpenCode)
	if st.Status != StatusHarnessChanged {
		t.Fatalf("harness edit = %s, want harness-changed (never stale): %+v", st.Status, st)
	}
	if len(st.Drift) == 0 {
		t.Fatalf("expected per-file drift rows, got none: %+v", st)
	}
	found := false
	for _, d := range st.Drift {
		if d.Path == "SKILL.md" && d.Kind == DriftHarnessChanged {
			found = true
		}
	}
	if !found {
		t.Fatalf("drift must name SKILL.md as harness-changed: %+v", st.Drift)
	}
}

// TestStatusLibraryEditStaysStale pins that the plain push case from #115
// still classifies as stale after the three-way refinement, and that a
// harness edit on top of a library edit (different content) is a conflict.
func TestStatusLibraryEditStaysStale(t *testing.T) {
	home := t.TempDir()
	lib := t.TempDir()
	writeLibrarySkill(t, lib, "pushcase")

	installFixture(t, home, lib, "pushcase", []render.Target{render.TargetOpenCode}, ModeCopy, false)

	// Library-only edit → stale (push direction), unchanged from #115.
	editLibrarySkill(t, lib, "pushcase")
	statuses, err := Status(statusOpts(home, lib))
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	if st := findByTarget(t, statuses, render.TargetOpenCode); st.Status != StatusStale {
		t.Fatalf("library edit = %s, want stale", st.Status)
	}

	// Now also edit the installed copy to different content → conflict.
	dest, err := InstallPath(render.TargetOpenCode, "pushcase", Options{HomeDir: home, Scope: render.ScopeUser})
	if err != nil {
		t.Fatal(err)
	}
	editInstalledCopy(t, dest)

	statuses, err = Status(statusOpts(home, lib))
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	st := findByTarget(t, statuses, render.TargetOpenCode)
	if st.Status != StatusConflict {
		t.Fatalf("diverged both sides = %s, want conflict (%+v)", st.Status, st)
	}
	if st.Error == "" {
		t.Fatal("conflict install must carry an error naming the files")
	}
}

// TestStatusDeletionAtHarnessIsHarnessChanged covers the deletion row of
// #126: removing SKILL.md from the installed copy (library intact) is a
// harness-side change, and the status scan must survive the missing file.
func TestStatusDeletionAtHarnessIsHarnessChanged(t *testing.T) {
	home := t.TempDir()
	lib := t.TempDir()
	writeLibrarySkill(t, lib, "delcase")

	installFixture(t, home, lib, "delcase", []render.Target{render.TargetOpenCode}, ModeCopy, false)

	dest, err := InstallPath(render.TargetOpenCode, "delcase", Options{HomeDir: home, Scope: render.ScopeUser})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.Remove(filepath.Join(dest, "SKILL.md")); err != nil {
		t.Fatal(err)
	}

	statuses, err := Status(statusOpts(home, lib))
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	st := findByTarget(t, statuses, render.TargetOpenCode)
	if st.Status != StatusHarnessChanged {
		t.Fatalf("deleted at harness = %s, want harness-changed", st.Status)
	}
	for _, d := range st.Drift {
		if d.Path == "SKILL.md" && d.Kind != DriftHarnessChanged {
			t.Fatalf("SKILL.md drift = %s, want harness-changed (deleted at harness)", d.Kind)
		}
	}
}

// TestStatusAdditionAtHarnessIsHarnessChanged covers the addition row: a
// support file created in the installed copy is a harness-side change.
func TestStatusAdditionAtHarnessIsHarnessChanged(t *testing.T) {
	home := t.TempDir()
	lib := t.TempDir()
	writeLibrarySkill(t, lib, "addcase")

	installFixture(t, home, lib, "addcase", []render.Target{render.TargetOpenCode}, ModeCopy, false)

	dest, err := InstallPath(render.TargetOpenCode, "addcase", Options{HomeDir: home, Scope: render.ScopeUser})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dest, "handmade.txt"), []byte("new\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	statuses, err := Status(statusOpts(home, lib))
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	st := findByTarget(t, statuses, render.TargetOpenCode)
	if st.Status != StatusHarnessChanged {
		t.Fatalf("added at harness = %s, want harness-changed", st.Status)
	}
}

// TestStatusConvergedIsInSync: when the installed copy is edited to exactly
// match the current library render, the install is content-in-sync even
// though the marker hash differs; sync must not reinstall it.
func TestStatusConvergedIsInSync(t *testing.T) {
	home := t.TempDir()
	lib := t.TempDir()
	writeLibrarySkill(t, lib, "convcase")

	installFixture(t, home, lib, "convcase", []render.Target{render.TargetOpenCode}, ModeCopy, false)

	// Library edit, then copy the fresh render over the installed tree so
	// right == left != base.
	editLibrarySkill(t, lib, "convcase")
	bundle, err := skill.LoadBundle(filepath.Join(lib, "convcase"))
	if err != nil {
		t.Fatal(err)
	}
	out := t.TempDir()
	rendered, errs := render.RenderAll(bundle, out, []render.Target{render.TargetOpenCode})
	if len(errs) > 0 || len(rendered) == 0 {
		t.Fatalf("render: %v", errs)
	}
	dest, err := InstallPath(render.TargetOpenCode, "convcase", Options{HomeDir: home, Scope: render.ScopeUser})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.RemoveAll(dest); err != nil {
		t.Fatal(err)
	}
	if err := copyDir(rendered[0].Path, dest); err != nil {
		t.Fatal(err)
	}

	statuses, err := Status(statusOpts(home, lib))
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	if st := findByTarget(t, statuses, render.TargetOpenCode); st.Status != StatusInSync {
		t.Fatalf("converged install = %s, want in-sync (%+v)", st.Status, st)
	}
}
