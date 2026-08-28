package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/adapter"
	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-brain/internal/skills/render"
	"github.com/danieljustus/symaira-brain/internal/skillsrunner"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// TestHarnessRegistryDerivedTables prevents a new harness from being added to
// one downstream capability table without being represented in the registry.
func TestHarnessRegistryDerivedTables(t *testing.T) {
	byName := make(map[string]harness.Harness, len(harness.All))
	for _, h := range harness.All {
		byName[string(h.Name)] = h
	}

	adapters := adapter.TargetsForHarnesses()
	for name := range adapters {
		if _, ok := byName[name]; !ok {
			t.Errorf("instruction adapter table contains unregistered harness %q", name)
		}
	}
	for name, target := range skillsrunner.HarnessMap {
		h, ok := byName[name]
		if !ok {
			t.Errorf("skill target table contains unregistered harness %q", name)
			continue
		}
		if string(h.SkillTarget) != string(target) {
			t.Errorf("%s: HarnessMap target %q disagrees with registry target %q", name, target, h.SkillTarget)
		}
	}
	rendered := make(map[render.Target]bool, len(render.Targets))
	for _, target := range render.Targets {
		if rendered[target.Name] {
			t.Errorf("render target %q is declared more than once", target.Name)
		}
		rendered[target.Name] = true
		if _, ok := byName[string(target.Name)]; !ok {
			t.Errorf("render target contains unregistered harness %q", target.Name)
		}
	}
	for _, h := range harness.All {
		if h.InstructionAdapter != harness.InstructionAdapterNone {
			if _, ok := adapters[string(h.Name)]; !ok {
				t.Errorf("%s: registry instruction adapter is not derived", h.Name)
			}
		}
		if h.SkillTarget != harness.SkillTargetNone && !rendered[render.Target(h.SkillTarget)] {
			t.Errorf("%s: registry skill target %q is absent from render targets", h.Name, h.SkillTarget)
		}
		if h.SkillTarget == harness.SkillTargetNone && rendered[render.Target(h.Name)] {
			t.Errorf("%s: render target exists without a registry SkillTarget capability", h.Name)
		}
	}
}

func TestCmdSyncUnknownHarnessIsRejected(t *testing.T) {
	sandboxHome(t)
	var stdout, stderr bytes.Buffer
	code := cmdSyncWithFormat([]string{"claudecode"}, &stdout, &stderr, output.FormatTable)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("cmdSyncWithFormat = %d, want %d", code, exitcodes.ExitNoInput)
	}
	if stdout.Len() != 0 {
		t.Fatalf("unknown harness wrote stdout: %q", stdout.String())
	}
	for _, name := range harness.Names() {
		if !strings.Contains(stderr.String(), name) {
			t.Errorf("stderr %q does not list valid harness %q", stderr.String(), name)
		}
	}
}

func TestCmdSyncValidHarnessWithoutInstructionAdapterIsSkipped(t *testing.T) {
	sandboxHome(t)
	project := t.TempDir()
	if err := os.WriteFile(filepath.Join(project, "AGENTS.md"), []byte("# test project\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	var stdout, stderr bytes.Buffer
	code := cmdSyncWithFormat([]string{"--project", project, "claude-desktop"}, &stdout, &stderr, output.FormatTable)
	if code != exitcodes.ExitOK {
		t.Fatalf("cmdSyncWithFormat = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	if !strings.Contains(stdout.String(), "claude-desktop:") || !strings.Contains(stdout.String(), "skipped") {
		t.Fatalf("stdout = %q, want skipped claude-desktop row", stdout.String())
	}
	if !strings.Contains(stdout.String(), "has no instruction adapter") {
		t.Fatalf("stdout = %q, want precise skip reason", stdout.String())
	}
}
