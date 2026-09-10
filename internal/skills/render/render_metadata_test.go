package render

import (
	"encoding/json"
	"github.com/danieljustus/symaira-brain/internal/skills/skill"
	"path/filepath"
	"strings"
	"testing"
)

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
