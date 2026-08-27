package discover

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

func TestDiscoverScannedHarnessRoots(t *testing.T) {
	tempHome := t.TempDir()

	// Create an unmanaged skill under ~/.config/opencode/skills/my-unmanaged-skill
	unmanagedDir := filepath.Join(tempHome, ".config", "opencode", "skills", "my-unmanaged-skill")
	if err := os.MkdirAll(unmanagedDir, 0o755); err != nil {
		t.Fatalf("failed to create dir: %v", err)
	}
	skillMD := "---\nname: my-unmanaged-skill\ndescription: Test unmanaged skill\n---\n\nSkill body"
	if err := os.WriteFile(filepath.Join(unmanagedDir, "SKILL.md"), []byte(skillMD), 0o644); err != nil {
		t.Fatalf("failed to write SKILL.md: %v", err)
	}

	// Create a managed skill under ~/.claude/skills/my-managed-skill
	managedDir := filepath.Join(tempHome, ".claude", "skills", "my-managed-skill")
	if err := os.MkdirAll(managedDir, 0o755); err != nil {
		t.Fatalf("failed to create dir: %v", err)
	}
	if err := os.WriteFile(filepath.Join(managedDir, "SKILL.md"), []byte(skillMD), 0o644); err != nil {
		t.Fatalf("failed to write SKILL.md: %v", err)
	}
	if err := os.WriteFile(filepath.Join(managedDir, ".symskills.json"), []byte("{}"), 0o644); err != nil {
		t.Fatalf("failed to write marker: %v", err)
	}

	candidates, err := DiscoverScanned(Options{
		HomeDir: tempHome,
		Scope:   render.ScopeUser,
	})
	if err != nil {
		t.Fatalf("DiscoverScanned failed: %v", err)
	}

	if len(candidates) != 2 {
		t.Fatalf("expected 2 candidates, got %d", len(candidates))
	}

	var opencodeCand, claudeCand *Candidate
	for i := range candidates {
		switch candidates[i].Target {
		case render.TargetOpenCode:
			opencodeCand = &candidates[i]
		case render.TargetClaude:
			claudeCand = &candidates[i]
		}
	}

	if opencodeCand == nil || claudeCand == nil {
		t.Fatalf("missing candidate for opencode or claude")
	}

	if opencodeCand.Managed {
		t.Errorf("expected opencode candidate to be unmanaged")
	}
	if opencodeCand.Status != "candidate" {
		t.Errorf("expected status 'candidate', got %q", opencodeCand.Status)
	}

	if !claudeCand.Managed {
		t.Errorf("expected claude candidate to be managed")
	}
	if claudeCand.Status != "managed" {
		t.Errorf("expected status 'managed', got %q", claudeCand.Status)
	}
}

func TestDiscoverScannedExplicitPath(t *testing.T) {
	tempDir := t.TempDir()
	skillDir := filepath.Join(tempDir, "custom-skill")
	if err := os.MkdirAll(skillDir, 0o755); err != nil {
		t.Fatalf("failed to create dir: %v", err)
	}
	skillMD := "---\nname: custom-skill\ndescription: Custom path skill\n---\n\nBody"
	if err := os.WriteFile(filepath.Join(skillDir, "SKILL.md"), []byte(skillMD), 0o644); err != nil {
		t.Fatalf("failed to write SKILL.md: %v", err)
	}

	candidates, err := DiscoverScanned(Options{
		HomeDir: tempDir,
		Paths:   []string{skillDir},
	})
	if err != nil {
		t.Fatalf("DiscoverScanned failed: %v", err)
	}

	if len(candidates) != 1 {
		t.Fatalf("expected 1 candidate, got %d", len(candidates))
	}

	c := candidates[0]
	if c.DisplayName != "custom-skill" {
		t.Errorf("expected display name 'custom-skill', got %q", c.DisplayName)
	}
	if !c.Valid {
		t.Errorf("expected valid candidate")
	}
	if c.Source != "explicit-path" {
		t.Errorf("expected source 'explicit-path', got %q", c.Source)
	}
}
