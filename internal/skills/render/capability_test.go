package render

import (
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/skill"
)

// snapshotRegistry restores the target registry after a test mutates it, so
// capability declarations do not leak between tests.
func snapshotRegistry(t *testing.T) {
	t.Helper()
	saved := make([]TargetSpec, len(Targets))
	copy(saved, Targets)
	for i := range saved {
		if saved[i].Capabilities != nil {
			clone := make(map[string]bool, len(saved[i].Capabilities))
			for k, v := range saved[i].Capabilities {
				clone[k] = v
			}
			saved[i].Capabilities = clone
		}
	}
	t.Cleanup(func() { Targets = saved })
}

// requiringBundle writes a skill declaring the given capability requirements.
func requiringBundle(t *testing.T, requires string) *skill.Bundle {
	t.Helper()
	root := filepath.Join(t.TempDir(), "needs-subagents")
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: needs-subagents
description: A skill that cannot work without harness-managed child agents.
---

# Needs Subagents

Fan the work out across child agents.
`)
	writeFile(t, filepath.Join(root, "symskills.toml"), "[skill]\nname = \"needs-subagents\"\nrequires = "+requires+"\n")
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}
	return bundle
}

func TestCapabilityStateHasThreeStates(t *testing.T) {
	snapshotRegistry(t)
	if err := DeclareCapabilities(map[string]map[string]bool{
		"codex": {CapSubagents: true, CapMCP: false},
	}); err != nil {
		t.Fatalf("DeclareCapabilities: %v", err)
	}

	cases := []struct {
		target Target
		cap    string
		want   string
	}{
		{TargetCodex, CapSubagents, CapabilitySupported},
		{TargetCodex, CapMCP, CapabilityUnsupported},
		{TargetCodex, CapHooks, CapabilityUnknown},
		{TargetOpenCode, CapSubagents, CapabilityUnknown},
	}
	for _, tc := range cases {
		if got := CapabilityState(tc.target, tc.cap); got != tc.want {
			t.Errorf("CapabilityState(%s, %s): got %s, want %s", tc.target, tc.cap, got, tc.want)
		}
	}
}

func TestDeclareCapabilitiesRejectsTypos(t *testing.T) {
	snapshotRegistry(t)
	if err := DeclareCapabilities(map[string]map[string]bool{"nope": {CapSubagents: true}}); err == nil {
		t.Error("expected an error for an unknown target")
	}
	if err := DeclareCapabilities(map[string]map[string]bool{"claude": {"subagant": true}}); err == nil {
		t.Error("expected an error for an unknown capability name")
	}
}

func TestCapabilityStatesCoversWholeVocabulary(t *testing.T) {
	states := CapabilityStates(TargetClaude)
	if len(states) != len(Capabilities) {
		t.Fatalf("expected %d entries, got %d", len(Capabilities), len(states))
	}
	if states[CapSubagents] != CapabilitySupported {
		t.Errorf("claude declares subagents; got %s", states[CapSubagents])
	}
	if states[CapHooks] != CapabilityUnknown {
		t.Errorf("undeclared capabilities must read as unknown, got %s", states[CapHooks])
	}
}

// TestRenderRefusesUnsupportedCapability is the core of "the feature must be
// able to say no": a harness that states it cannot do what the skill needs
// gets no install at all, rather than one that claims compatibility.
func TestRenderRefusesUnsupportedCapability(t *testing.T) {
	snapshotRegistry(t)
	if err := DeclareCapabilities(map[string]map[string]bool{"codex": {CapSubagents: false}}); err != nil {
		t.Fatalf("DeclareCapabilities: %v", err)
	}

	_, err := RenderTarget(requiringBundle(t, `["subagents"]`), TargetCodex)
	if err == nil {
		t.Fatal("expected a refused render")
	}
	for _, want := range []string{"codex", "subagents", "--ignore-capabilities"} {
		if !strings.Contains(err.Error(), want) {
			t.Errorf("error must mention %q: %v", want, err)
		}
	}
}

func TestRenderForcedPastUnsupportedCapabilityClaimsNothing(t *testing.T) {
	snapshotRegistry(t)
	if err := DeclareCapabilities(map[string]map[string]bool{"codex": {CapSubagents: false}}); err != nil {
		t.Fatalf("DeclareCapabilities: %v", err)
	}

	item, err := RenderTarget(requiringBundle(t, `["subagents"]`), TargetCodex, RenderMeta{IgnoreCapabilities: true})
	if err != nil {
		t.Fatalf("forced render: %v", err)
	}
	if item.Frontmatter.Compatibility != "" {
		t.Errorf("a forced render must not claim compatibility, got %q", item.Frontmatter.Compatibility)
	}
	if strings.Contains(item.SkillMD, "compatibility:") {
		t.Errorf("compatibility must be absent from the rendered frontmatter:\n%s", item.SkillMD)
	}
	if len(item.UnmetRequirements) != 1 || item.UnmetRequirements[0].Capability != CapSubagents {
		t.Errorf("unmet requirements: got %+v", item.UnmetRequirements)
	}
	if len(item.Warnings) == 0 {
		t.Error("a forced render must warn")
	}
}

// TestRenderWarnsOnUndeclaredCapability: undeclared is missing information,
// not evidence of absence, so the render proceeds and says what is missing.
func TestRenderWarnsOnUndeclaredCapability(t *testing.T) {
	item, err := RenderTarget(requiringBundle(t, `["subagents"]`), TargetOpenCode)
	if err != nil {
		t.Fatalf("expected a render, got %v", err)
	}
	if item.Frontmatter.Compatibility != string(TargetOpenCode) {
		t.Errorf("compatibility: got %q", item.Frontmatter.Compatibility)
	}
	if len(item.Warnings) != 1 {
		t.Fatalf("expected one warning, got %v", item.Warnings)
	}
	if !strings.Contains(item.Warnings[0], "capabilities.opencode") {
		t.Errorf("warning must name the config fix: %s", item.Warnings[0])
	}
}

func TestRenderSatisfiedRequirementIsSilent(t *testing.T) {
	item, err := RenderTarget(requiringBundle(t, `["subagents"]`), TargetClaude)
	if err != nil {
		t.Fatalf("RenderTarget: %v", err)
	}
	if len(item.Warnings) != 0 {
		t.Errorf("a satisfied requirement must not warn: %v", item.Warnings)
	}
	if item.Frontmatter.Compatibility != string(TargetClaude) {
		t.Errorf("compatibility: got %q", item.Frontmatter.Compatibility)
	}
}

func TestRenderRejectsUnknownCapabilityName(t *testing.T) {
	_, err := RenderTarget(requiringBundle(t, `["telepathy"]`), TargetClaude)
	if err == nil || !strings.Contains(err.Error(), "telepathy") {
		t.Fatalf("expected an error naming the unknown capability, got %v", err)
	}
}

// TestRenderDropsForeignTargetMetadata: metadata namespaced under a harness
// name is that harness's convention and must not travel to the others.
func TestRenderDropsForeignTargetMetadata(t *testing.T) {
	root := filepath.Join(t.TempDir(), "tagged")
	writeFile(t, filepath.Join(root, "SKILL.md"), `---
name: tagged
description: A skill carrying target-namespaced metadata.
metadata:
  hermes:
    tags:
      - GitHub
  claude:
    tags:
      - Local
  workflow: github
---

# Tagged
`)
	bundle, err := skill.LoadBundle(root)
	if err != nil {
		t.Fatalf("LoadBundle: %v", err)
	}

	hermes, err := RenderTarget(bundle, TargetHermes)
	if err != nil {
		t.Fatalf("RenderTarget(hermes): %v", err)
	}
	if _, ok := hermes.Frontmatter.Metadata["hermes"]; !ok {
		t.Error("hermes render must keep its own namespace")
	}
	if _, ok := hermes.Frontmatter.Metadata["claude"]; ok {
		t.Error("hermes render must not carry the claude namespace")
	}
	if hermes.Frontmatter.Metadata["workflow"] != "github" {
		t.Error("non-target metadata keys must survive untouched")
	}

	codex, err := RenderTarget(bundle, TargetCodex)
	if err != nil {
		t.Fatalf("RenderTarget(codex): %v", err)
	}
	for _, foreign := range []string{"hermes", "claude"} {
		if _, ok := codex.Frontmatter.Metadata[foreign]; ok {
			t.Errorf("codex render must not carry the %s namespace", foreign)
		}
	}
	if codex.Frontmatter.Metadata["workflow"] != "github" {
		t.Error("non-target metadata keys must survive untouched")
	}
}

// TestRenderWithoutRequiresIsUnchanged is the backward-compatibility
// statement for phase 4: a skill that declares nothing behaves as before.
func TestRenderWithoutRequiresIsUnchanged(t *testing.T) {
	snapshotRegistry(t)
	if err := DeclareCapabilities(map[string]map[string]bool{"codex": {CapSubagents: false}}); err != nil {
		t.Fatalf("DeclareCapabilities: %v", err)
	}

	item, err := RenderTarget(requiringBundle(t, `[]`), TargetCodex)
	if err != nil {
		t.Fatalf("RenderTarget: %v", err)
	}
	if item.Frontmatter.Compatibility != string(TargetCodex) {
		t.Errorf("compatibility: got %q", item.Frontmatter.Compatibility)
	}
	if len(item.Warnings) != 0 {
		t.Errorf("expected no warnings, got %v", item.Warnings)
	}
}
