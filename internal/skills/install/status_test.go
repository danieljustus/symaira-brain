package install

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

// writeLibrarySkill creates a library skill bundle with a support file.
func writeLibrarySkill(t *testing.T, lib, name string) {
	t.Helper()
	dir := filepath.Join(lib, name)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "SKILL.md"), []byte("---\nname: "+name+"\ndescription: status test\n---\n\n# Body\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "notes.txt"), []byte("v1\n"), 0o644); err != nil {
		t.Fatal(err)
	}
}

// editLibrarySkill appends a line to the library SKILL.md, simulating a
// source edit after install.
func editLibrarySkill(t *testing.T, lib, name string) {
	t.Helper()
	path := filepath.Join(lib, name, "SKILL.md")
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, append(data, []byte("\n# v2\n")...), 0o644); err != nil {
		t.Fatal(err)
	}
}

// installFixture renders the library skill and installs it for every target
// using the install pipeline directly (the same code path the CLI runs).
func installFixture(t *testing.T, home, lib, name string, targets []render.Target, mode Mode, allowExec bool) {
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
			BaseDir:         filepath.Join(home, ".local", "share", "symskills", "base"),
			AllowExecutable: allowExec,
		}); err != nil {
			t.Fatalf("install %s/%s: %v", item.Target, item.Name, err)
		}
	}
}

func statusOpts(home, lib string) StatusOptions {
	return StatusOptions{
		HomeDir:    home,
		Scope:      render.ScopeUser,
		LibraryDir: lib,
		BaseDir:    filepath.Join(home, ".local", "share", "symskills", "base"),
	}
}

func findByTarget(t *testing.T, statuses []InstallStatus, target render.Target) InstallStatus {
	t.Helper()
	for _, st := range statuses {
		if st.Target == target {
			return st
		}
	}
	t.Fatalf("no status entry for target %s in %+v", target, statuses)
	return InstallStatus{}
}

// TestStatusRoundTripTwoTargets pins the source-hash contract: the hash
// Status computes must match what install wrote, or every install would
// report permanently stale (#115 risk). Editing the library source must
// flip both targets to stale, and re-install must return both to in-sync.
func TestStatusRoundTripTwoTargets(t *testing.T) {
	home := t.TempDir()
	lib := t.TempDir()
	writeLibrarySkill(t, lib, "roundtrip")

	installFixture(t, home, lib, "roundtrip", []render.Target{render.TargetOpenCode, render.TargetClaude}, ModeSymlink, false)

	statuses, err := Status(statusOpts(home, lib))
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	if len(statuses) != 2 {
		t.Fatalf("expected 2 installs, got %+v", statuses)
	}
	for _, st := range statuses {
		if st.Status != StatusInSync {
			t.Errorf("expected in-sync after fresh install, got %+v", st)
		}
		if st.SourceHash == "" {
			t.Errorf("expected source_hash on the marker, got %+v", st)
		}
	}

	// A library edit must make both targets stale.
	editLibrarySkill(t, lib, "roundtrip")
	statuses, err = Status(statusOpts(home, lib))
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	for _, st := range statuses {
		if st.Status != StatusStale {
			t.Errorf("expected stale after library edit, got %+v", st)
		}
	}

	// Re-install must return both to in-sync (the hash computation matches
	// what install writes).
	installFixture(t, home, lib, "roundtrip", []render.Target{render.TargetOpenCode, render.TargetClaude}, ModeSymlink, false)
	statuses, err = Status(statusOpts(home, lib))
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	for _, st := range statuses {
		if st.Status != StatusInSync {
			t.Errorf("expected in-sync after re-install, got %+v", st)
		}
	}
}

// TestStatusReportsInstallPathAndMode pins the acceptance requirement that
// status names target, skill and install path for stale installs.
func TestStatusReportsInstallPathAndMode(t *testing.T) {
	home := t.TempDir()
	lib := t.TempDir()
	writeLibrarySkill(t, lib, "pathed")
	installFixture(t, home, lib, "pathed", []render.Target{render.TargetOpenCode}, ModeCopy, false)
	editLibrarySkill(t, lib, "pathed")

	statuses, err := Status(statusOpts(home, lib))
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	st := findByTarget(t, statuses, render.TargetOpenCode)
	if st.Status != StatusStale {
		t.Fatalf("expected stale, got %+v", st)
	}
	wantPath := filepath.Join(home, ".config", "opencode", "skills", "pathed")
	if st.Path != wantPath {
		t.Errorf("install path = %q, want %q", st.Path, wantPath)
	}
	if st.Mode != ModeCopy {
		t.Errorf("mode = %q, want %q", st.Mode, ModeCopy)
	}
	if st.Name != "pathed" {
		t.Errorf("name = %q, want pathed", st.Name)
	}
}

// TestStatusOrphaned pins that a managed install whose library source was
// deleted is reported orphaned and left untouched.
func TestStatusOrphaned(t *testing.T) {
	home := t.TempDir()
	lib := t.TempDir()
	writeLibrarySkill(t, lib, "orphan")
	installFixture(t, home, lib, "orphan", []render.Target{render.TargetOpenCode}, ModeCopy, false)

	// Delete the library source.
	if err := os.RemoveAll(filepath.Join(lib, "orphan")); err != nil {
		t.Fatal(err)
	}

	statuses, err := Status(statusOpts(home, lib))
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	st := findByTarget(t, statuses, render.TargetOpenCode)
	if st.Status != StatusOrphaned {
		t.Fatalf("expected orphaned, got %+v", st)
	}
	// The install must still exist and still carry its marker.
	dest := filepath.Join(home, ".config", "opencode", "skills", "orphan")
	if _, err := os.Stat(filepath.Join(dest, markerFile)); err != nil {
		t.Fatalf("orphaned install was modified: %v", err)
	}
}

// TestStatusUnmanaged pins that hand-installed skills (no marker) are
// reported unmanaged and never classified as drift.
func TestStatusUnmanaged(t *testing.T) {
	home := t.TempDir()
	lib := t.TempDir()
	handmade := filepath.Join(home, ".config", "opencode", "skills", "handmade")
	if err := os.MkdirAll(handmade, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(handmade, "SKILL.md"), []byte("---\nname: handmade\ndescription: hand-written\n---\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	statuses, err := Status(statusOpts(home, lib))
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	if len(statuses) != 1 {
		t.Fatalf("expected exactly one entry, got %+v", statuses)
	}
	if statuses[0].Status != StatusUnmanaged || statuses[0].Name != "handmade" {
		t.Fatalf("expected unmanaged handmade entry, got %+v", statuses[0])
	}
}

// TestStatusLegacyMarkerWithoutHash pins the fallback for installs that
// predate source hashes: the content comparison (install.Diff) decides,
// so a matching copy is in-sync and a diverging one is stale.
func TestStatusLegacyMarkerWithoutHash(t *testing.T) {
	home := t.TempDir()
	lib := t.TempDir()
	writeLibrarySkill(t, lib, "legacy")
	installFixture(t, home, lib, "legacy", []render.Target{render.TargetOpenCode}, ModeCopy, false)

	// Simulate an install written before source hashes existed: drop the
	// source_hash field from the installed marker.
	dest := filepath.Join(home, ".config", "opencode", "skills", "legacy")
	markerPath := filepath.Join(dest, markerFile)
	data, err := os.ReadFile(markerPath)
	if err != nil {
		t.Fatal(err)
	}
	var m map[string]any
	if err := json.Unmarshal(data, &m); err != nil {
		t.Fatal(err)
	}
	delete(m, "source_hash")
	rewritten, err := json.MarshalIndent(m, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(markerPath, append(rewritten, '\n'), 0o644); err != nil {
		t.Fatal(err)
	}

	statuses, err := Status(statusOpts(home, lib))
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	st := findByTarget(t, statuses, render.TargetOpenCode)
	if st.Status != StatusInSync {
		t.Fatalf("expected in-sync via content fallback, got %+v", st)
	}

	editLibrarySkill(t, lib, "legacy")
	statuses, err = Status(statusOpts(home, lib))
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	st = findByTarget(t, statuses, render.TargetOpenCode)
	if st.Status != StatusStale {
		t.Fatalf("expected stale via content fallback, got %+v", st)
	}
}

// TestStatusSkillFilter pins the --skill filter.
func TestStatusSkillFilter(t *testing.T) {
	home := t.TempDir()
	lib := t.TempDir()
	writeLibrarySkill(t, lib, "alpha")
	writeLibrarySkill(t, lib, "beta")
	installFixture(t, home, lib, "alpha", []render.Target{render.TargetOpenCode}, ModeCopy, false)
	installFixture(t, home, lib, "beta", []render.Target{render.TargetOpenCode}, ModeCopy, false)

	opts := statusOpts(home, lib)
	opts.Skills = []string{"alpha"}
	statuses, err := Status(opts)
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	if len(statuses) != 1 || statuses[0].Name != "alpha" {
		t.Fatalf("expected only alpha, got %+v", statuses)
	}
}

// TestInstallMarkerRecordsAllowExecutable pins the additive marker field
// that lets sync replay the original install's executable policy.
func TestInstallMarkerRecordsAllowExecutable(t *testing.T) {
	home := t.TempDir()
	lib := t.TempDir()
	writeLibrarySkill(t, lib, "execmark")
	installFixture(t, home, lib, "execmark", []render.Target{render.TargetOpenCode}, ModeCopy, true)

	dest := filepath.Join(home, ".config", "opencode", "skills", "execmark")
	m, ok, err := readInstallMarker(dest)
	if err != nil || !ok {
		t.Fatalf("readInstallMarker: ok=%v err=%v", ok, err)
	}
	if !m.AllowExecutable {
		t.Fatal("expected allow_executable recorded in the marker")
	}
	if !strings.Contains(string(mustReadFile(t, filepath.Join(dest, markerFile))), "allow_executable") {
		t.Fatal("expected allow_executable field in serialized marker")
	}
}

func mustReadFile(t *testing.T, path string) []byte {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return data
}

// TestStatusHarnessChangedWithNonDefaultBaseDir verifies that a harness-side
// edit is classified as harness-changed (not stale) when a non-default
// BaseDir is used. This is a regression test for #219: the MCP path must
// thread BaseDir into install.StatusOptions, otherwise the base snapshot
// lookup misses and the three-way classification degrades to the coarse
// source-hash comparison that cannot distinguish harness edits from stale.
func TestStatusHarnessChangedWithNonDefaultBaseDir(t *testing.T) {
	home := t.TempDir()
	lib := t.TempDir()

	// Use a non-default base dir, as a user with a custom base_dir config
	// would have.
	customBase := filepath.Join(home, ".custom", "base")

	writeLibrarySkill(t, lib, "mcp-base")
	installFixtureCustomBase(t, home, lib, "mcp-base", []render.Target{render.TargetOpenCode}, ModeCopy, customBase)

	// Harness-side edit: modify the installed copy.
	dest := filepath.Join(home, ".config", "opencode", "skills", "mcp-base", "SKILL.md")
	data, err := os.ReadFile(dest)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(dest, append(data, []byte("\n# harness edit\n")...), 0o644); err != nil {
		t.Fatal(err)
	}

	statuses, err := Status(StatusOptions{
		HomeDir:    home,
		Scope:      render.ScopeUser,
		LibraryDir: lib,
		BaseDir:    customBase,
	})
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	st := findByTarget(t, statuses, render.TargetOpenCode)
	if st.Status != StatusHarnessChanged {
		t.Fatalf("expected harness-changed with non-default BaseDir, got %s (error: %s)", st.Status, st.Error)
	}

	// Negative control: with the wrong (default) BaseDir, the base snapshot
	// is not found, and the three-way classification degrades to the coarse
	// source-hash comparison — which cannot see the harness-side edit and
	// reports in-sync (the data-loss risk from #219: sync would not flag
	// the edit, and resync could overwrite it).
	wrongBase := filepath.Join(home, ".local", "share", "symskills", "base")
	statusesWrong, err := Status(StatusOptions{
		HomeDir:    home,
		Scope:      render.ScopeUser,
		LibraryDir: lib,
		BaseDir:    wrongBase,
	})
	if err != nil {
		t.Fatalf("Status: %v", err)
	}
	stWrong := findByTarget(t, statusesWrong, render.TargetOpenCode)
	if stWrong.Status == StatusHarnessChanged {
		t.Fatalf("expected NOT harness-changed with wrong BaseDir (bug from #219), got harness-changed")
	}
}

// installFixtureCustomBase is like installFixture but uses an explicit base
// directory instead of the default.
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
