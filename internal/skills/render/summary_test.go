package render

import (
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

func summaryFor(t *testing.T, summaries []VariantSummary, target Target) VariantSummary {
	t.Helper()
	for _, summary := range summaries {
		if summary.Target == target {
			return summary
		}
	}
	t.Fatalf("no summary for target %s", target)
	return VariantSummary{}
}

func TestSummarizeReportsPerTargetDivergence(t *testing.T) {
	bundle := loadVariantBundle(t, variantBundle(t))
	summaries := Summarize(bundle, []Target{TargetClaude, TargetHermes})

	claude := summaryFor(t, summaries, TargetClaude)
	if claude.Status != StatusRendered {
		t.Fatalf("claude status: got %s", claude.Status)
	}
	if claude.Divergence <= 0 || claude.Divergence >= 1 {
		t.Errorf("claude divergence out of range: %v", claude.Divergence)
	}
	if len(claude.Blocks) != 2 {
		t.Errorf("claude blocks: got %v", claude.Blocks)
	}

	hermes := summaryFor(t, summaries, TargetHermes)
	if len(hermes.Blocks) != 0 {
		t.Errorf("hermes overrides no block: got %v", hermes.Blocks)
	}
	if hermes.Divergence >= claude.Divergence {
		t.Errorf("the target that replaces more text must diverge more: hermes=%v claude=%v", hermes.Divergence, claude.Divergence)
	}
	if got := MaxDivergence(summaries); got != claude.Divergence {
		t.Errorf("MaxDivergence: got %v, want %v", got, claude.Divergence)
	}
}

// TestSummarizeReportsDisabledAndRefused: one unrunnable target must not hide
// the picture for the others.
func TestSummarizeReportsDisabledAndRefused(t *testing.T) {
	snapshotRegistry(t)
	if err := DeclareCapabilities(map[string]map[string]bool{"codex": {CapSubagents: false}}); err != nil {
		t.Fatalf("DeclareCapabilities: %v", err)
	}

	root := filepath.Join(t.TempDir(), "mixed")
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: mixed
description: A skill that is disabled for one target and refused by another.
---

# Mixed
`)
	writeFile(t, filepath.Join(root, "symskills.toml"), `[skill]
name = "mixed"
requires = ["subagents"]

[targets.opencode]
enabled = false
`)
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}

	summaries := Summarize(bundle, []Target{TargetOpenCode, TargetCodex, TargetClaude})

	if got := summaryFor(t, summaries, TargetOpenCode); got.Status != StatusDisabled {
		t.Errorf("opencode: got %s (%s)", got.Status, got.Reason)
	}
	codex := summaryFor(t, summaries, TargetCodex)
	if codex.Status != StatusRefused {
		t.Errorf("codex: got %s", codex.Status)
	}
	if codex.Reason == "" {
		t.Error("a refusal must carry its reason")
	}
	if got := summaryFor(t, summaries, TargetClaude); got.Status != StatusRendered {
		t.Errorf("claude: got %s (%s)", got.Status, got.Reason)
	}
	if !HasVariants(summaries) {
		t.Error("a disabled or refused target is worth reporting")
	}
}

func TestSummarizePlainSkillHasNothingToReport(t *testing.T) {
	root := filepath.Join(t.TempDir(), "plain")
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: plain
description: An ordinary portable skill with no harness variants.
---

# Plain
`)
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}
	summaries := Summarize(bundle, nil)
	if len(summaries) != len(DefaultTargets()) {
		t.Fatalf("expected one summary per target, got %d", len(summaries))
	}
	if HasVariants(summaries) {
		t.Errorf("a plain skill must have nothing to report: %+v", summaries)
	}
	if MaxDivergence(summaries) != 0 {
		t.Errorf("MaxDivergence: got %v, want 0", MaxDivergence(summaries))
	}
}

func TestSummarizeCarriesCapabilityWarnings(t *testing.T) {
	root := filepath.Join(t.TempDir(), "needs")
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: needs
description: A skill requiring a capability opencode has not declared.
---

# Needs
`)
	writeFile(t, filepath.Join(root, "symskills.toml"), "[skill]\nname = \"needs\"\nrequires = [\"subagents\"]\n")
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}
	summary := summaryFor(t, Summarize(bundle, []Target{TargetOpenCode}), TargetOpenCode)
	if summary.Status != StatusRendered {
		t.Fatalf("an undeclared capability renders: got %s", summary.Status)
	}
	if len(summary.Warnings) == 0 {
		t.Error("the warning must survive into the summary")
	}
}

func TestSummarizeNilBundle(t *testing.T) {
	if got := Summarize(nil, nil); got != nil {
		t.Errorf("expected nil for a nil bundle, got %+v", got)
	}
}
