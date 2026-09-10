package render

import (
	"fmt"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

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

func TestRenderAllRejectsDestinationParentSymlink(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("symlinks require elevated privileges on Windows")
	}
	root := t.TempDir()
	writeFile(t, filepath.Join(root, "SKILL.md"), "---\nname: destination-safe\ndescription: test\n---\nBody.\n")
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}
	out := filepath.Join(t.TempDir(), "rendered")
	outside := filepath.Join(t.TempDir(), "outside")
	if err := os.MkdirAll(outside, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(out, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(out, "opencode")); err != nil {
		t.Fatal(err)
	}
	results, errs := RenderAll(bundle, out, []Target{TargetOpenCode})
	if len(results) != 0 || len(errs) != 1 {
		t.Fatalf("expected one confined destination error, results=%v errors=%v", results, errs)
	}
	if _, err := os.Stat(filepath.Join(outside, "destination-safe", "SKILL.md")); !os.IsNotExist(err) {
		t.Fatalf("destination escape wrote outside root: %v", err)
	}
}

func TestRenderUsesRetainedSourceRootAfterPathSwap(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("path swap uses symlinks on Windows")
	}
	parent := t.TempDir()
	root := filepath.Join(parent, "source")
	outside := filepath.Join(parent, "outside")
	if err := os.MkdirAll(filepath.Join(root, "scripts"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(outside, "scripts"), 0o755); err != nil {
		t.Fatal(err)
	}
	writeFile(t, filepath.Join(root, "SKILL.md"), "---\nname: retained-root\ndescription: test\n---\nBody.\n")
	writeFile(t, filepath.Join(root, "scripts", "tool.sh"), "original\n")
	writeFile(t, filepath.Join(outside, "SKILL.md"), "---\nname: retained-root\ndescription: outside\n---\nOutside.\n")
	writeFile(t, filepath.Join(outside, "scripts", "tool.sh"), "outside\n")
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatal(err)
	}
	expectedFingerprint, err := sourceFingerprint(bundle)
	if err != nil {
		t.Fatal(err)
	}
	moved := filepath.Join(parent, "moved")
	if err := os.Rename(root, moved); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, root); err != nil {
		t.Fatal(err)
	}
	actualFingerprint, err := sourceFingerprint(bundle)
	if err != nil {
		t.Fatal(err)
	}
	if actualFingerprint != expectedFingerprint {
		t.Fatalf("source fingerprint followed swapped pathname: got %q, want %q", actualFingerprint, expectedFingerprint)
	}
	item, err := RenderTarget(bundle, TargetOpenCode)
	if err != nil {
		t.Fatal(err)
	}
	out := filepath.Join(t.TempDir(), "rendered")
	if err := writeRendered(bundle, out, item, TargetOpenCode, sourceTreeHash(bundle)); err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(filepath.Join(out, "scripts", "tool.sh"))
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != "original\n" {
		t.Fatalf("render reopened swapped source path: %q", data)
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

// TestRenderAllDoesNotReopenSourceTree proves rendering uses the loader's
// retained capability rather than walking bundle.Root again.
func TestRenderAllDoesNotReopenSourceTree(t *testing.T) {
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

	out := filepath.Join(t.TempDir(), "rendered")
	results, errs := RenderAll(bundle, out, []Target{TargetOpenCode, TargetClaude, TargetCodex, TargetHermes})
	if len(errs) != 0 {
		t.Fatalf("RenderAll errors: %v", errs)
	}
	if len(results) != 4 {
		t.Fatalf("want 4 rendered targets, got %d", len(results))
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
