package harness

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/skills/render"
)

func TestListStatusUserScope(t *testing.T) {
	tempHome := t.TempDir()

	// Pre-create opencode skills dir with 1 managed skill and 1 unmanaged skill
	opencodeRoot := filepath.Join(tempHome, ".config", "opencode", "skills")
	if err := os.MkdirAll(filepath.Join(opencodeRoot, "managed-skill"), 0o755); err != nil {
		t.Fatalf("failed to create dir: %v", err)
	}
	if err := os.WriteFile(filepath.Join(opencodeRoot, "managed-skill", ".symskills.json"), []byte("{}"), 0o644); err != nil {
		t.Fatalf("failed to write marker: %v", err)
	}

	if err := os.MkdirAll(filepath.Join(opencodeRoot, "unmanaged-skill"), 0o755); err != nil {
		t.Fatalf("failed to create dir: %v", err)
	}

	statuses := ListStatus(Options{
		HomeDir: tempHome,
		Scope:   render.ScopeUser,
	})

	if len(statuses) != len(Descriptors()) {
		t.Fatalf("expected %d statuses, got %d", len(Descriptors()), len(statuses))
	}

	var opencodeStatus *Status
	for i := range statuses {
		if statuses[i].Target == render.TargetOpenCode {
			opencodeStatus = &statuses[i]
			break
		}
	}

	if opencodeStatus == nil {
		t.Fatalf("opencode status not found")
	}

	if !opencodeStatus.SkillRootExists {
		t.Errorf("expected skill root to exist")
	}
	if !opencodeStatus.SkillRootReadable {
		t.Errorf("expected skill root to be readable")
	}
	if opencodeStatus.ManagedSkillsCount != 1 {
		t.Errorf("expected 1 managed skill, got %d", opencodeStatus.ManagedSkillsCount)
	}
	if opencodeStatus.UnmanagedSkillsCount != 1 {
		t.Errorf("expected 1 unmanaged skill, got %d", opencodeStatus.UnmanagedSkillsCount)
	}
	if opencodeStatus.InstallState != "mixed" {
		t.Errorf("expected install state 'mixed', got %q", opencodeStatus.InstallState)
	}
}

func TestListStatusMissingRoots(t *testing.T) {
	tempHome := t.TempDir()

	statuses := ListStatus(Options{
		HomeDir: tempHome,
		Scope:   render.ScopeUser,
	})

	for _, s := range statuses {
		if s.SkillRootExists {
			t.Errorf("expected skill root not to exist for %s", s.Target)
		}
		if s.InstallState != "missing" {
			t.Errorf("expected install state 'missing', got %q for %s", s.InstallState, s.Target)
		}
	}
}
