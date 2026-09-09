package render

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

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

func TestRenderTargetRejectsUnknownTargetBeforeValidation(t *testing.T) {
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), "---\nname: unknown-target\ndescription: test\n---\nBody.\n")
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}
	_, err = RenderTarget(bundle, Target("not-registered"))
	if err == nil || err.Error() != `unknown target "not-registered"` {
		t.Fatalf("unknown target error = %v", err)
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
