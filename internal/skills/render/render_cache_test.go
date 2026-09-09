package render

import (
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestWriteRenderedPreservesInstallMarker(t *testing.T) {
	// Regression test for #65: re-rendering a directory that contains
	// a .symskills.json marker must preserve its fields. Also verifies
	// that a second render with unchanged source is skipped (#87).
	tmp := t.TempDir()
	marker := filepath.Join(tmp, ".symskills.json")
	if err := os.WriteFile(marker, []byte(`{"installed":true}`), 0o644); err != nil {
		t.Fatal(err)
	}

	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), "---\nname: marker-test\ndescription: test\n---\n# Body\n")
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	item := Rendered{Name: "marker-test", SkillMD: "---\nname: marker-test\ndescription: test\n---\n# Body\n"}
	treeHash := sourceTreeHash(bundle)

	// First render: must write the marker with source_hash added.
	if err := writeRendered(bundle, tmp, item, TargetOpenCode, treeHash); err != nil {
		t.Fatalf("writeRendered (first): %v", err)
	}

	data, err := os.ReadFile(marker)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(data), `"installed": true`) {
		t.Fatalf("installed field lost: %q", string(data))
	}
	if !strings.Contains(string(data), `"source_hash"`) {
		t.Fatalf("source_hash missing from marker: %q", string(data))
	}

	// Capture file modification times after first render.
	skillMDInfo, err := os.Stat(filepath.Join(tmp, "SKILL.md"))
	if err != nil {
		t.Fatal(err)
	}
	firstModTime := skillMDInfo.ModTime()

	// Second render: source is unchanged, should skip the rewrite (#87).
	if err := writeRendered(bundle, tmp, item, TargetOpenCode, treeHash); err != nil {
		t.Fatalf("writeRendered (second): %v", err)
	}

	skillMDInfo2, err := os.Stat(filepath.Join(tmp, "SKILL.md"))
	if err != nil {
		t.Fatal(err)
	}
	if !skillMDInfo2.ModTime().Equal(firstModTime) {
		t.Fatalf("second render modified output despite unchanged source: mod time changed from %v to %v",
			firstModTime, skillMDInfo2.ModTime())
	}
}

// --- Tests for #87: render-cache freshness ---

func TestWriteRenderedSkipsUnchangedSource(t *testing.T) {
	// A second writeRendered with unchanged source must leave the
	// output untouched (no RemoveAll, no fresh copy).
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), "---\nname: skip-test\ndescription: test\n---\n# Body\n")
	writeFile(t, filepath.Join(root, "scripts", "tool.sh"), "#!/bin/sh\necho ok\n")
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	item := Rendered{Name: "skip-test", SkillMD: "---\nname: skip-test\ndescription: test\n---\n# Body\n"}
	treeHash := sourceTreeHash(bundle)

	out := t.TempDir()

	// First render: fresh output.
	if err := writeRendered(bundle, out, item, TargetOpenCode, treeHash); err != nil {
		t.Fatalf("first writeRendered: %v", err)
	}

	// Record mod times of all output files.
	modTimes := fileModTimes(t, out)
	if len(modTimes) == 0 {
		t.Fatal("no output files after first render")
	}

	// Second render: source unchanged, must skip.
	if err := writeRendered(bundle, out, item, TargetOpenCode, treeHash); err != nil {
		t.Fatalf("second writeRendered: %v", err)
	}

	modTimes2 := fileModTimes(t, out)
	for path, mt1 := range modTimes {
		mt2, ok := modTimes2[path]
		if !ok {
			t.Fatalf("file %q disappeared after second render", path)
		}
		if !mt1.Equal(mt2) {
			t.Fatalf("file %q was rewritten despite unchanged source: %v -> %v", path, mt1, mt2)
		}
	}
}

func TestWriteRenderedFullRenderOnChangedSource(t *testing.T) {
	// Changing the source must trigger a full re-render with updated output.
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), "---\nname: change-test\ndescription: test\n---\n# Original body\n")
	writeFile(t, filepath.Join(root, "scripts", "tool.sh"), "#!/bin/sh\necho v1\n")

	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	out := t.TempDir()

	// First render.
	item1, err := RenderTarget(bundle, TargetOpenCode)
	if err != nil {
		t.Fatal(err)
	}
	if err := writeRendered(bundle, out, item1, TargetOpenCode, sourceTreeHash(bundle)); err != nil {
		t.Fatal(err)
	}
	modTimes := fileModTimes(t, out)

	// Modify the source.
	writeFile(t, filepath.Join(root, "SKILL.md"), "---\nname: change-test\ndescription: test\n---\n# Changed body\n")
	writeFile(t, filepath.Join(root, "scripts", "tool.sh"), "#!/bin/sh\necho v2\n")

	bundle2, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}
	item2, err := RenderTarget(bundle2, TargetOpenCode)
	if err != nil {
		t.Fatal(err)
	}

	// Second render: source changed, must trigger full rewrite.
	if err := writeRendered(bundle2, out, item2, TargetOpenCode, sourceTreeHash(bundle2)); err != nil {
		t.Fatal(err)
	}

	// Verify output was updated.
	modTimes2 := fileModTimes(t, out)
	changed := false
	for path, mt1 := range modTimes {
		mt2, ok := modTimes2[path]
		if !ok {
			continue
		}
		if !mt1.Equal(mt2) {
			changed = true
			break
		}
	}
	if !changed {
		t.Fatal("expected output files to be rewritten after source change")
	}
}

func TestWriteRenderedFullRenderOnMissingMarker(t *testing.T) {
	// When the output directory exists but has no .symskills.json marker,
	// a full render must still happen (no crash, no skip).
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), "---\nname: no-marker\ndescription: test\n---\n# Body\n")
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	item := Rendered{Name: "no-marker", SkillMD: "---\nname: no-marker\ndescription: test\n---\n# Body\n"}

	out := t.TempDir()
	// Pre-populate the output dir with a SKILL.md file but NO marker.
	if err := os.WriteFile(filepath.Join(out, "SKILL.md"), []byte("old-content"), 0o644); err != nil {
		t.Fatal(err)
	}

	if err := writeRendered(bundle, out, item, TargetOpenCode, sourceTreeHash(bundle)); err != nil {
		t.Fatalf("writeRendered with missing marker: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(out, "SKILL.md"))
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != item.SkillMD {
		t.Fatalf("SKILL.md not updated: got %q, want %q", string(data), item.SkillMD)
	}
	// Marker must have been created.
	if _, err := os.Stat(filepath.Join(out, ".symskills.json")); os.IsNotExist(err) {
		t.Fatal("marker was not created after full render")
	}
}

func fileModTimes(t *testing.T, dir string) map[string]time.Time {
	t.Helper()
	out := map[string]time.Time{}
	err := filepath.WalkDir(dir, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			return nil
		}
		rel, err := filepath.Rel(dir, path)
		if err != nil {
			return err
		}
		info, err := d.Info()
		if err != nil {
			return err
		}
		out[rel] = info.ModTime()
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	return out
}

func TestParseTargetErrorListsValidNames(t *testing.T) {
	_, err := ParseTarget("invalid-target")
	if err == nil {
		t.Fatal("expected error for invalid target")
	}
	msg := err.Error()
	for _, name := range []string{"opencode", "claude", "codex", "hermes", "antigravity", "openclaw"} {
		if !strings.Contains(msg, name) {
			t.Errorf("error message missing valid name %q: %s", name, msg)
		}
	}
	if !strings.Contains(msg, "all") {
		t.Errorf("error message should mention 'all' as a special value: %s", msg)
	}
}
