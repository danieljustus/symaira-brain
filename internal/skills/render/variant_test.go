package render

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

// variantBundle writes a skill whose execution contract is harness-bound in
// the canonical body and in a markdown reference — the shape the real
// issue-sweep skill has.
func variantBundle(t *testing.T) string {
	t.Helper()
	root := filepath.Join(t.TempDir(), "sweeper")
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: sweeper
description: Clear a repository backlog through isolated workers.
---

# Sweeper

<!-- symskills:block worker-isolation -->
Hermes only. Dispatch each worker with delegate_task at the configured tier.
<!-- /symskills:block -->

Persist the run report under {{term:report_dir}}/sweeper/.

<!-- symskills:only hermes -->
Never invoke an external coding-agent executable.
<!-- /symskills:only -->
`)
	writeFile(t, filepath.Join(root, "symskills.toml"), `[skill]
name = "sweeper"
version = "1.0.0"

[targets.hermes]
enabled = true

[targets.claude]
enabled = true

[terms.report_dir]
default = "~/.local/state/symskills/reports"
hermes = "~/.hermes/reports"
`)
	writeFile(t, filepath.Join(root, "references", "execution-contract.md"), `# Execution contract

<!-- symskills:block dispatch -->
Use delegate_task children with the strong tier.
<!-- /symskills:block -->

Reports land in {{term:report_dir}}.
`)
	writeFile(t, filepath.Join(root, "scripts", "helper.sh"), "#!/bin/sh\necho '{{term:report_dir}}'\n")
	writeFile(t, filepath.Join(root, "overlays", "claude", "blocks", "worker-isolation.md"),
		"Dispatch each worker with the Agent tool in its own git worktree.\n")
	writeFile(t, filepath.Join(root, "overlays", "claude", "blocks", "dispatch.md"),
		"Use one Agent subagent per group, each in an isolated worktree.\n")
	return root
}

func loadVariantBundle(t *testing.T, root string) *skill.Bundle {
	t.Helper()
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}
	return bundle
}

// TestRenderTargetSubstitutesBlocksAndTerms is the end-to-end statement of the
// feature: the Claude render must not carry the Hermes execution contract, and
// the Hermes render must keep it — from one source.
func TestRenderTargetSubstitutesBlocksAndTerms(t *testing.T) {
	bundle := loadVariantBundle(t, variantBundle(t))

	claude, err := RenderTarget(bundle, TargetClaude)
	if err != nil {
		t.Fatalf("RenderTarget(claude): %v", err)
	}
	for _, forbidden := range []string{"delegate_task", "Hermes only", "~/.hermes/reports", "Never invoke an external"} {
		if strings.Contains(claude.SkillMD, forbidden) {
			t.Errorf("claude render still carries %q:\n%s", forbidden, claude.SkillMD)
		}
	}
	if !strings.Contains(claude.SkillMD, "Agent tool in its own git worktree") {
		t.Errorf("claude render missing its own contract:\n%s", claude.SkillMD)
	}
	if !strings.Contains(claude.SkillMD, "~/.local/state/symskills/reports/sweeper/") {
		t.Errorf("claude render missing the default term value:\n%s", claude.SkillMD)
	}

	hermes, err := RenderTarget(bundle, TargetHermes)
	if err != nil {
		t.Fatalf("RenderTarget(hermes): %v", err)
	}
	if !strings.Contains(hermes.SkillMD, "delegate_task at the configured tier") {
		t.Errorf("hermes render lost its canonical contract:\n%s", hermes.SkillMD)
	}
	if !strings.Contains(hermes.SkillMD, "~/.hermes/reports/sweeper/") {
		t.Errorf("hermes render missing its term override:\n%s", hermes.SkillMD)
	}
	if !strings.Contains(hermes.SkillMD, "Never invoke an external coding-agent executable") {
		t.Errorf("hermes-only region must survive for hermes:\n%s", hermes.SkillMD)
	}

	for _, item := range []Rendered{claude, hermes} {
		if strings.Contains(item.SkillMD, "symskills:") {
			t.Errorf("%s: markers must never reach a rendered skill:\n%s", item.Target, item.SkillMD)
		}
		if strings.Contains(item.SkillMD, "{{term:") {
			t.Errorf("%s: unresolved term placeholder in output:\n%s", item.Target, item.SkillMD)
		}
	}
}

func TestRenderTargetReportsVariants(t *testing.T) {
	bundle := loadVariantBundle(t, variantBundle(t))

	claude, err := RenderTarget(bundle, TargetClaude)
	if err != nil {
		t.Fatalf("RenderTarget(claude): %v", err)
	}
	if claude.Variants == nil {
		t.Fatal("expected a variant report for a skill that uses blocks and terms")
	}
	if got := strings.Join(claude.Variants.Blocks, ","); got != "dispatch,worker-isolation" {
		t.Errorf("blocks: got %q", got)
	}
	if claude.Variants.Terms["report_dir"] != "~/.local/state/symskills/reports" {
		t.Errorf("terms: got %v", claude.Variants.Terms)
	}
	if got := strings.Join(claude.Variants.Files, ","); got != "references/execution-contract.md" {
		t.Errorf("files: got %q", got)
	}
	if claude.Variants.SourceBytes == 0 || claude.Variants.ReplacedBytes == 0 {
		t.Errorf("divergence figures missing: %+v", claude.Variants)
	}
}

// TestRenderWritesResolvedReferences proves the resolution reaches the files a
// harness actually loads, not only SKILL.md.
func TestRenderWritesResolvedReferences(t *testing.T) {
	bundle := loadVariantBundle(t, variantBundle(t))
	out := t.TempDir()

	rendered, errs := RenderAll(bundle, out, []Target{TargetClaude, TargetHermes})
	if len(errs) > 0 {
		t.Fatalf("RenderAll: %v", errs)
	}
	if len(rendered) != 2 {
		t.Fatalf("expected 2 rendered targets, got %d", len(rendered))
	}

	claudeRef := readFile(t, filepath.Join(out, "claude", "sweeper", "references", "execution-contract.md"))
	if strings.Contains(claudeRef, "delegate_task") || strings.Contains(claudeRef, "~/.hermes") {
		t.Errorf("claude reference still harness-bound:\n%s", claudeRef)
	}
	if !strings.Contains(claudeRef, "one Agent subagent per group") {
		t.Errorf("claude reference missing its override:\n%s", claudeRef)
	}

	hermesRef := readFile(t, filepath.Join(out, "hermes", "sweeper", "references", "execution-contract.md"))
	if !strings.Contains(hermesRef, "delegate_task children with the strong tier") {
		t.Errorf("hermes reference lost its canonical text:\n%s", hermesRef)
	}
	if !strings.Contains(hermesRef, "Reports land in ~/.hermes/reports") {
		t.Errorf("hermes reference missing its term override:\n%s", hermesRef)
	}

	// Non-markdown resources travel byte-identical: a placeholder-looking
	// string in a script is never rewritten.
	script := readFile(t, filepath.Join(out, "claude", "sweeper", "scripts", "helper.sh"))
	if !strings.Contains(script, "{{term:report_dir}}") {
		t.Errorf("script content must not be resolved:\n%s", script)
	}

	// Overlay input never ships.
	if _, err := os.Stat(filepath.Join(out, "claude", "sweeper", "overlays")); !os.IsNotExist(err) {
		t.Error("overlays/ must not be copied into a rendered skill")
	}
}

// TestSourceHashCoversResolvedReferences guards the cache: an overlay edit
// that only changes a reference must still invalidate the rendered output.
func TestSourceHashCoversResolvedReferences(t *testing.T) {
	root := variantBundle(t)
	out := t.TempDir()

	if _, errs := RenderAll(loadVariantBundle(t, root), out, []Target{TargetClaude}); len(errs) > 0 {
		t.Fatalf("first render: %v", errs)
	}
	first := markerHash(t, filepath.Join(out, "claude", "sweeper", ".symskills.json"))

	writeFile(t, filepath.Join(root, "overlays", "claude", "blocks", "dispatch.md"),
		"Use two Agent subagents per group, each in an isolated worktree.\n")
	if _, errs := RenderAll(loadVariantBundle(t, root), out, []Target{TargetClaude}); len(errs) > 0 {
		t.Fatalf("second render: %v", errs)
	}
	second := markerHash(t, filepath.Join(out, "claude", "sweeper", ".symskills.json"))

	if first == second {
		t.Error("editing a block override that only affects a reference must change source_hash")
	}
	ref := readFile(t, filepath.Join(out, "claude", "sweeper", "references", "execution-contract.md"))
	if !strings.Contains(ref, "two Agent subagents") {
		t.Errorf("rendered reference is stale:\n%s", ref)
	}
}

// TestSourceHashUnchangedWithoutVariants is the backward-compatibility
// statement: a skill that uses no blocks and no terms must keep the exact
// source_hash it had before harness variants existed, so no install is
// reported stale by the upgrade alone.
func TestSourceHashUnchangedWithoutVariants(t *testing.T) {
	item := Rendered{SkillMD: "---\nname: plain\n---\n\nBody\n"}
	withoutFeature := legacySourceHash("tree", item.SkillMD, TargetClaude)
	withFeature := sourceHash("tree", item.SkillMD, TargetClaude, nil)
	if withoutFeature != withFeature {
		t.Errorf("hash of a variant-free skill changed: %s != %s", withoutFeature, withFeature)
	}
	if sourceHash("tree", item.SkillMD, TargetClaude, map[string]string{"a.md": "x"}) == withFeature {
		t.Error("resolved files must contribute to the hash")
	}
}

func TestRenderRefusesOverrideForUnknownBlock(t *testing.T) {
	root := variantBundle(t)
	writeFile(t, filepath.Join(root, "overlays", "claude", "blocks", "invented.md"), "text\n")

	_, err := RenderTarget(loadVariantBundle(t, root), TargetClaude)
	if err == nil {
		t.Fatal("expected a refused render for an override with no matching block")
	}
	if !strings.Contains(err.Error(), "invented") {
		t.Errorf("error must name the offending block: %v", err)
	}
}

// TestRenderRefusesMalformedMarkerInOverlay covers the path skill.Validate
// cannot see: overlay prepend/append text is not part of the bundle body.
func TestRenderRefusesMalformedMarkerInOverlay(t *testing.T) {
	root := variantBundle(t)
	writeFile(t, filepath.Join(root, "overlays", "claude", "prepend.md"), "<!-- symskills:blok typo -->\n")

	_, err := RenderTarget(loadVariantBundle(t, root), TargetClaude)
	if err == nil {
		t.Fatal("expected a refused render for a malformed marker in an overlay fragment")
	}
	if !strings.Contains(err.Error(), "symskills:blok") {
		t.Errorf("error must quote the offending marker: %v", err)
	}
}

func TestRenderPlainSkillHasNoVariantReport(t *testing.T) {
	root := filepath.Join(t.TempDir(), "plain")
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: plain
description: A skill that uses no harness variants at all.
---

# Plain

Nothing target-specific here.
`)
	item, err := RenderTarget(loadVariantBundle(t, root), TargetClaude)
	if err != nil {
		t.Fatalf("RenderTarget: %v", err)
	}
	if item.Variants != nil {
		t.Errorf("expected no variant report, got %+v", item.Variants)
	}
	if len(item.Files) != 0 {
		t.Errorf("expected no resolved files, got %v", item.Files)
	}
}

// legacySourceHash is the source_hash algorithm as it stood before harness
// variants existed. Keeping it here lets the compatibility test compare
// against the old contract instead of against the new implementation.
func legacySourceHash(treeHash, renderedSkillMD string, target Target) string {
	h := sha256.New()
	h.Write([]byte(treeHash))
	h.Write([]byte{0})
	h.Write([]byte(renderedSkillMD))
	h.Write([]byte{0})
	h.Write([]byte(target))
	return hex.EncodeToString(h.Sum(nil))
}

func readFile(t *testing.T, path string) string {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	return string(data)
}

func markerHash(t *testing.T, path string) string {
	t.Helper()
	var marker map[string]any
	if err := json.Unmarshal([]byte(readFile(t, path)), &marker); err != nil {
		t.Fatalf("parse marker %s: %v", path, err)
	}
	hash, _ := marker["source_hash"].(string)
	if hash == "" {
		t.Fatalf("marker %s has no source_hash", path)
	}
	return hash
}
