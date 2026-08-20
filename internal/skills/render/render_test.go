package render

import (
	"encoding/json"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

func TestRenderTargetAppliesOverlayAndTargetFrontmatter(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: overlaid
description: Base description for render tests.
license: Apache-2.0
metadata:
  workflow: base
---

# Base Body

Use the base workflow.
`)
	writeFile(t, filepath.Join(root, "symskills.toml"), `[skill]
name = "overlaid"
version = "0.2.0"

[targets.opencode]
enabled = true
alias = "overlaid-open"
description = "OpenCode-specific description."
`)
	writeFile(t, filepath.Join(root, "overlays", "opencode", "prepend.md"), "## OpenCode Note\n\nLoad guard skills first.\n")
	writeFile(t, filepath.Join(root, "overlays", "opencode", "append.md"), "## OpenCode Tail\n\nReport next skill.\n")
	writeFile(t, filepath.Join(root, "overlays", "opencode", "frontmatter.toml"), `[metadata]
workflow = "github"
audience = "maintainers"
`)

	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	rendered, err := RenderTarget(bundle, TargetOpenCode)
	if err != nil {
		t.Fatalf("RenderTarget: %v", err)
	}

	if rendered.Name != "overlaid-open" {
		t.Fatalf("rendered name: want alias, got %q", rendered.Name)
	}
	if rendered.Frontmatter.Description != "OpenCode-specific description." {
		t.Fatalf("description override missing: %q", rendered.Frontmatter.Description)
	}
	if rendered.Frontmatter.Compatibility != "opencode" {
		t.Fatalf("compatibility: want opencode, got %q", rendered.Frontmatter.Compatibility)
	}
	if rendered.Frontmatter.Metadata["audience"] != "maintainers" {
		t.Fatalf("overlay metadata missing: %#v", rendered.Frontmatter.Metadata)
	}
	if !strings.Contains(rendered.SkillMD, "## OpenCode Note") || !strings.Contains(rendered.SkillMD, "## OpenCode Tail") {
		t.Fatalf("overlay body fragments missing:\n%s", rendered.SkillMD)
	}
}

func TestRenderAllWritesHarnessReadableSkillFolders(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: basic
description: Basic render fixture.
---

Body.
`)
	writeFile(t, filepath.Join(root, "scripts", "helper.sh"), "#!/bin/sh\n")

	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	out := filepath.Join(t.TempDir(), "rendered")
	results, errs := RenderAll(bundle, out, []Target{TargetOpenCode, TargetClaude, TargetCodex, TargetHermes})
	if len(errs) != 0 {
		t.Fatalf("RenderAll errors: %v", errs)
	}
	if len(results) != 4 {
		t.Fatalf("want 4 rendered targets, got %d", len(results))
	}
	for _, result := range results {
		if _, err := os.Stat(filepath.Join(result.Path, "SKILL.md")); err != nil {
			t.Fatalf("%s SKILL.md missing: %v", result.Target, err)
		}
		if _, err := os.Stat(filepath.Join(result.Path, "scripts", "helper.sh")); err != nil {
			t.Fatalf("%s copied support file missing: %v", result.Target, err)
		}
	}
	if _, err := os.Stat(filepath.Join(out, "codex", "basic", "agents", "openai.yaml")); err != nil {
		t.Fatalf("codex metadata file missing: %v", err)
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

func TestRenderTargetRejectsHostileResolvedNames(t *testing.T) {
	cases := []struct {
		name            string
		frontmatterName string
		manifest        string
		overlay         string
	}{
		{
			name:            "alias_with_path_traversal",
			frontmatterName: "safe",
			manifest: `[skill]
name = "safe"
version = "0.1.0"

[targets.opencode]
enabled = true
alias = "../../evil"
`,
		},
		{
			name:            "manifest_name_with_separator",
			frontmatterName: "safe",
			manifest: `[skill]
name = "evil/name"
version = "0.1.0"

[targets.opencode]
enabled = true
`,
		},
		{
			name:            "overlay_name_with_path_traversal",
			frontmatterName: "safe",
			manifest: `[skill]
name = "safe"
version = "0.1.0"

[targets.opencode]
enabled = true
`,
			overlay: `name = "../evil"
`,
		},
		{
			name:            "frontmatter_name_absolute_path",
			frontmatterName: "/etc/evil",
			manifest: `[skill]
version = "0.1.0"

[targets.opencode]
enabled = true
`,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			root := t.TempDir()
			writeFile(t, filepath.Join(root, "SKILL.md"), fmt.Sprintf(`---
name: %s
description: Test.
---

Body.
`, tc.frontmatterName))
			writeFile(t, filepath.Join(root, "symskills.toml"), tc.manifest)
			if tc.overlay != "" {
				writeFile(t, filepath.Join(root, "overlays", "opencode", "frontmatter.toml"), tc.overlay)
			}

			bundle, err := skill.LoadBundle(root)
			if err != nil {
				t.Fatal(err)
			}

			_, err = RenderTarget(bundle, TargetOpenCode)
			if err == nil {
				t.Fatal("expected error for hostile resolved name")
			}
		})
	}
}

func TestRenderAllReportsPerTargetErrors(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: error-test
description: Test.
---

Body.
`)
	writeFile(t, filepath.Join(root, "symskills.toml"), `[skill]
name = "error-test"
version = "0.1.0"

[targets.opencode]
enabled = false

[targets.claude]
enabled = true
alias = "../../evil"

[targets.codex]
enabled = true

[targets.hermes]
enabled = false
`)

	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	out := filepath.Join(t.TempDir(), "rendered")
	results, errs := RenderAll(bundle, out, []Target{TargetOpenCode, TargetClaude, TargetCodex, TargetHermes})
	if len(results) != 1 {
		t.Fatalf("want 1 successful render (codex), got %d", len(results))
	}
	if results[0].Target != TargetCodex {
		t.Fatalf("want codex success, got %s", results[0].Target)
	}
	if len(errs) != 3 {
		t.Fatalf("want 3 errors (opencode disabled, claude hostile, hermes disabled), got %d: %v", len(errs), errs)
	}
}

// TestRenderAllComputesSourceTreeHashOncePerBundle proves the source tree is
// walked exactly once per bundle, regardless of how many targets render.
func TestRenderAllComputesSourceTreeHashOncePerBundle(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: hash-once
description: Hash once per bundle test.
---
Body.
`)
	writeFile(t, filepath.Join(root, "scripts", "tool.sh"), "#!/bin/sh\necho ok\n")
	writeFile(t, filepath.Join(root, "references", "guide.md"), "# Guide\n")

	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	origWalk := walkBundleDir
	walks := 0
	walkBundleDir = func(root string, fn fs.WalkDirFunc) error {
		walks++
		return origWalk(root, fn)
	}
	defer func() { walkBundleDir = origWalk }()

	out := filepath.Join(t.TempDir(), "rendered")
	results, errs := RenderAll(bundle, out, []Target{TargetOpenCode, TargetClaude, TargetCodex, TargetHermes})
	if len(errs) != 0 {
		t.Fatalf("RenderAll errors: %v", errs)
	}
	if len(results) != 4 {
		t.Fatalf("want 4 rendered targets, got %d", len(results))
	}
	if walks != 1 {
		t.Fatalf("source tree walked %d times for 4 targets; want exactly 1 (once per bundle)", walks)
	}
}

// --- Tests for #82: profile alias rendering ---

func TestRenderTargetProfileAliasOverridesTargetConfigAlias(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: my-skill
description: Test skill.
---
# Body
`)
	writeFile(t, filepath.Join(root, "symskills.toml"), `[skill]
name = "my-skill"
version = "0.1.0"

[targets.opencode]
enabled = true
alias = "target-alias"
`)

	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	// Profile alias wins over target config alias.
	rendered, err := RenderTarget(bundle, TargetOpenCode, RenderMeta{Alias: "profile-override"})
	if err != nil {
		t.Fatalf("RenderTarget: %v", err)
	}
	if rendered.Name != "profile-override" {
		t.Fatalf("rendered name: want profile-override, got %q", rendered.Name)
	}
}

func TestRenderTargetProfileAliasFallsBackToTargetConfig(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: my-skill
description: Fallback test.
---
# Body
`)
	writeFile(t, filepath.Join(root, "symskills.toml"), `[skill]
name = "my-skill"
version = "0.1.0"

[targets.opencode]
enabled = true
alias = "target-alias"
`)

	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	// Without profile alias, target config alias is used.
	rendered, err := RenderTarget(bundle, TargetOpenCode, RenderMeta{Source: "project", Profile: "default"})
	if err != nil {
		t.Fatalf("RenderTarget: %v", err)
	}
	if rendered.Name != "target-alias" {
		t.Fatalf("rendered name: want target-alias, got %q", rendered.Name)
	}
}

func TestRenderTargetInvalidProfileAliasFailsValidation(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: my-skill
description: Invalid alias test.
---
# Body
`)
	writeFile(t, filepath.Join(root, "symskills.toml"), `[skill]
name = "my-skill"
version = "0.1.0"

[targets.opencode]
enabled = true
`)

	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	// An invalid profile alias must fail with ValidateSkillName error.
	_, err = RenderTarget(bundle, TargetOpenCode, RenderMeta{Alias: "../../evil"})
	if err == nil {
		t.Fatal("expected error for invalid profile alias")
	}
	if !strings.Contains(err.Error(), "invalid resolved name") {
		t.Fatalf("expected ValidateSkillName error, got: %v", err)
	}
}

func TestRenderTargetEmptyProfileAliasNoEffect(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: my-skill
description: Empty alias test.
---
# Body
`)
	writeFile(t, filepath.Join(root, "symskills.toml"), `[skill]
name = "my-skill"
version = "0.1.0"

[targets.opencode]
enabled = true
`)

	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	// Empty profile alias should fall back to manifest name.
	rendered, err := RenderTarget(bundle, TargetOpenCode, RenderMeta{Alias: ""})
	if err != nil {
		t.Fatalf("RenderTarget: %v", err)
	}
	if rendered.Name != "my-skill" {
		t.Fatalf("rendered name: want my-skill, got %q", rendered.Name)
	}
}

func TestParseTarget(t *testing.T) {
	valid := []Target{TargetOpenCode, TargetClaude, TargetCodex, TargetHermes, TargetAntigravity, TargetOpenClaw}
	for _, target := range valid {
		got, err := ParseTarget(string(target))
		if err != nil {
			t.Fatalf("expected target %q to parse successfully, got: %v", target, err)
		}
		if got != target {
			t.Errorf("ParseTarget(%q) = %q, want %q", target, got, target)
		}
	}

	invalid := []string{"", "invalid-target", "open-code", "CLAUDE"}
	for _, s := range invalid {
		_, err := ParseTarget(s)
		if err == nil {
			t.Fatalf("expected error parsing invalid target %q, but got nil", s)
		}
	}
}

func TestRenderTargetCarriesProvenanceMetadata(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: meta-test
description: Meta test.
---

Body.
`)
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	rendered, err := RenderTarget(bundle, TargetOpenCode, RenderMeta{Source: "project", Profile: "default"})
	if err != nil {
		t.Fatalf("RenderTarget: %v", err)
	}
	if rendered.Source != "project" {
		t.Fatalf("source: want project, got %q", rendered.Source)
	}
	if rendered.Profile != "default" {
		t.Fatalf("profile: want default, got %q", rendered.Profile)
	}

	var decoded Rendered
	if err := jsonRoundTrip(rendered, &decoded); err != nil {
		t.Fatalf("JSON round-trip: %v", err)
	}
	if decoded.Source != "project" || decoded.Profile != "default" {
		t.Fatalf("JSON decoded metadata mismatch: %+v", decoded)
	}
}

func TestRenderTargetWithoutMetaOmitsProvenanceFields(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: no-meta
description: No meta.
---

Body.
`)
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	rendered, err := RenderTarget(bundle, TargetOpenCode)
	if err != nil {
		t.Fatalf("RenderTarget: %v", err)
	}
	if rendered.Source != "" || rendered.Profile != "" {
		t.Fatalf("expected empty source/profile, got %+v", rendered)
	}

	var decoded Rendered
	if err := jsonRoundTrip(rendered, &decoded); err != nil {
		t.Fatalf("JSON round-trip: %v", err)
	}
	if decoded.Source != "" || decoded.Profile != "" {
		t.Fatalf("JSON decoded metadata should be empty, got %+v", decoded)
	}
}

func jsonRoundTrip(in, out any) error {
	data, err := json.Marshal(in)
	if err != nil {
		return err
	}
	return json.Unmarshal(data, out)
}

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

	item := Rendered{Name: "marker-test", SkillMD: "---\nname: marker-test\ndescription: test\n---\n# Body\n"}
	treeHash := sourceTreeHash(root)

	// First render: must write the marker with source_hash added.
	if err := writeRendered(root, tmp, item, TargetOpenCode, treeHash); err != nil {
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
	if err := writeRendered(root, tmp, item, TargetOpenCode, treeHash); err != nil {
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

// --- Regression tests for #80: overlay path traversal hardening ---

func TestRenderTargetRejectsTraversalOverlayPrepend(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: traversal-prepend
description: Test traversal in prepend.
---
Body.
`)
	writeFile(t, filepath.Join(root, "symskills.toml"), `[skill]
name = "traversal-prepend"

[targets.opencode]
enabled = true
prepend = "../../evil.md"
`)

	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	_, err = RenderTarget(bundle, TargetOpenCode)
	if err == nil {
		t.Fatal("expected error for traversal prepend, got nil")
	}
	if !strings.Contains(err.Error(), "escapes") && !strings.Contains(err.Error(), "must be relative") {
		t.Fatalf("expected containment error, got: %v", err)
	}
}

func TestRenderTargetRejectsTraversalOverlayAppend(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: traversal-append
description: Test traversal in append.
---
Body.
`)
	writeFile(t, filepath.Join(root, "symskills.toml"), `[skill]
name = "traversal-append"

[targets.opencode]
enabled = true
append = "../evil.md"
`)

	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	_, err = RenderTarget(bundle, TargetOpenCode)
	if err == nil {
		t.Fatal("expected error for traversal append, got nil")
	}
	if !strings.Contains(err.Error(), "escapes") && !strings.Contains(err.Error(), "must be relative") {
		t.Fatalf("expected containment error, got: %v", err)
	}
}

func TestRenderTargetRejectsAbsoluteOverlayPrepend(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: absolute-prepend
description: Test absolute path in prepend.
---
Body.
`)
	writeFile(t, filepath.Join(root, "symskills.toml"), `[skill]
name = "absolute-prepend"

[targets.opencode]
enabled = true
prepend = "/etc/passwd"
`)

	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	_, err = RenderTarget(bundle, TargetOpenCode)
	if err == nil {
		t.Fatal("expected error for absolute prepend, got nil")
	}
	if !strings.Contains(err.Error(), "must be relative") {
		t.Fatalf("expected 'must be relative' error, got: %v", err)
	}
}

func TestRenderTargetAcceptsValidConfiguredOverlay(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: valid-configured
description: Valid configured overlay.
---
Base body.
`)
	writeFile(t, filepath.Join(root, "custom-prepend.md"), "## Custom Prepend\n\nExtra content.\n")
	writeFile(t, filepath.Join(root, "symskills.toml"), `[skill]
name = "valid-configured"

[targets.opencode]
enabled = true
prepend = "custom-prepend.md"
`)

	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}

	rendered, err := RenderTarget(bundle, TargetOpenCode)
	if err != nil {
		t.Fatalf("expected valid overlay to succeed, got: %v", err)
	}
	if !strings.Contains(rendered.SkillMD, "## Custom Prepend") {
		t.Fatalf("expected custom prepend in output, got:\n%s", rendered.SkillMD)
	}
}

// --- Tests for #87: render-cache freshness ---

func TestWriteRenderedSkipsUnchangedSource(t *testing.T) {
	// A second writeRendered with unchanged source must leave the
	// output untouched (no RemoveAll, no fresh copy).
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), "---\nname: skip-test\ndescription: test\n---\n# Body\n")
	writeFile(t, filepath.Join(root, "scripts", "tool.sh"), "#!/bin/sh\necho ok\n")

	item := Rendered{Name: "skip-test", SkillMD: "---\nname: skip-test\ndescription: test\n---\n# Body\n"}
	treeHash := sourceTreeHash(root)

	out := t.TempDir()

	// First render: fresh output.
	if err := writeRendered(root, out, item, TargetOpenCode, treeHash); err != nil {
		t.Fatalf("first writeRendered: %v", err)
	}

	// Record mod times of all output files.
	modTimes := fileModTimes(t, out)
	if len(modTimes) == 0 {
		t.Fatal("no output files after first render")
	}

	// Second render: source unchanged, must skip.
	if err := writeRendered(root, out, item, TargetOpenCode, treeHash); err != nil {
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
	if err := writeRendered(root, out, item1, TargetOpenCode, sourceTreeHash(root)); err != nil {
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
	if err := writeRendered(root, out, item2, TargetOpenCode, sourceTreeHash(root)); err != nil {
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

	item := Rendered{Name: "no-marker", SkillMD: "---\nname: no-marker\ndescription: test\n---\n# Body\n"}

	out := t.TempDir()
	// Pre-populate the output dir with a SKILL.md file but NO marker.
	if err := os.WriteFile(filepath.Join(out, "SKILL.md"), []byte("old-content"), 0o644); err != nil {
		t.Fatal(err)
	}

	if err := writeRendered(root, out, item, TargetOpenCode, sourceTreeHash(root)); err != nil {
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
