package harness

import (
	"os"
	"path/filepath"
	"testing"
)

func TestIsManagedSkillMarkerPresent(t *testing.T) {
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, ".symskills.json"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}
	if !isManagedSkill(dir) {
		t.Fatal("directory with marker must be managed")
	}
}

func TestIsManagedSkillSymlinkTargetMarker(t *testing.T) {
	target := t.TempDir()
	if err := os.WriteFile(filepath.Join(target, ".symskills.json"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(t.TempDir(), "linked")
	if err := os.Symlink(target, link); err != nil {
		t.Fatal(err)
	}
	if !isManagedSkill(link) {
		t.Fatal("symlink whose target has a marker must be managed")
	}
}

func TestIsManagedSkillPlainDirectory(t *testing.T) {
	if isManagedSkill(t.TempDir()) {
		t.Fatal("plain directory must not be managed")
	}
}

func TestIsManagedSkillSymlinkWithoutMarker(t *testing.T) {
	target := t.TempDir()
	link := filepath.Join(t.TempDir(), "linked")
	if err := os.Symlink(target, link); err != nil {
		t.Fatal(err)
	}
	if isManagedSkill(link) {
		t.Fatal("symlink without marker in target must not be managed")
	}
}
