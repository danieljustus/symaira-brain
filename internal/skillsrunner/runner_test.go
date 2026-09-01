package skillsrunner

import (
	"context"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

const validSkillMD = `---
name: demo
description: A demo skill for tests.
---

# Demo

Body.
`

// disabledSkillTOML turns off the claude target for the disabled skill,
// exactly as a real symskills.toml would.
const disabledSkillTOML = `[targets.claude]
enabled = false
`

const invalidSkillMD = `---
name: broken
description: A skill that fails to render.
---

<!-- symskills:blok typo -->

# Bad

Body.
`

// writeLibrary builds a temp HOME with the same XDG layout the released
// binary uses, so DefaultOptions() resolves inside the sandbox.
func writeLibrary(t *testing.T) Options {
	t.Helper()
	home := t.TempDir()
	t.Setenv("HOME", home)

	opts := DefaultOptions()
	opts.HomeDir = home
	for _, dir := range []string{opts.LibraryDir, opts.RenderDir, opts.BaseDir} {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatal(err)
		}
	}
	return opts
}

func writeSkill(t *testing.T, opts Options, name, content string) {
	t.Helper()
	writeSkillFiles(t, opts, name, map[string]string{"SKILL.md": content})
}

// writeSkillFiles writes a skill directory with the given relative files,
// defaulting to a plain marker-free SKILL.md when the map omits it.
func writeSkillFiles(t *testing.T, opts Options, name string, files map[string]string) {
	t.Helper()
	dir := filepath.Join(opts.LibraryDir, name)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	if _, ok := files["SKILL.md"]; !ok {
		files["SKILL.md"] = validSkillMD
	}
	for rel, content := range files {
		if err := os.WriteFile(filepath.Join(dir, rel), []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
}

// TestRun_NoLegacyBinaryProcessesTargets is the core no-legacy-binary path:
// with symskills stripped from PATH, the same planned targets are still
// processed in-process, and the installed skill dirs exist. The disabled
// skill is not installed.
func TestRun_NoLegacyBinaryProcessesTargets(t *testing.T) {
	opts := writeLibrary(t)
	writeSkill(t, opts, "demo", validSkillMD)
	writeSkillFiles(t, opts, "noclient", map[string]string{"symskills.toml": disabledSkillTOML})

	// PATH points at an empty dir: no symskills anywhere.
	t.Setenv("PATH", t.TempDir())

	results, err := Run(context.Background(), []string{"claude", "codex", "hermes", "opencode"}, opts, false)
	if err != nil {
		t.Fatalf("Run() error: %v", err)
	}
	if len(results) != 4 {
		t.Fatalf("expected 4 results, got %d: %+v", len(results), results)
	}
	for _, r := range results {
		if r.Status == "error" {
			t.Errorf("%s: status %q (%s), want ok", r.Target, r.Status, r.Message)
		}
		if r.Status == "skipped" {
			t.Errorf("%s: status %q, want not skipped", r.Target, r.Status)
		}
	}

	for _, skillDir := range []string{
		filepath.Join(opts.HomeDir, ".claude", "skills"),
		filepath.Join(opts.HomeDir, ".agents", "skills"), // codex
		filepath.Join(opts.HomeDir, ".config", "opencode", "skills"),
	} {
		if _, err := os.Stat(filepath.Join(skillDir, "demo")); os.IsNotExist(err) {
			t.Errorf("expected demo installed under %s", skillDir)
		}
		if _, err := os.Stat(filepath.Join(skillDir, "noclient")); err == nil {
			t.Errorf("disabled skill noclient must not be installed under %s", skillDir)
		}
	}
}

// runBaseline builds a library with one skill and returns the planned
// results for claude+codex. Used to prove binary presence is irrelevant.
func runBaseline(t *testing.T) []Result {
	t.Helper()
	opts := writeLibrary(t)
	writeSkill(t, opts, "demo", validSkillMD)
	results, err := Run(context.Background(), []string{"claude", "codex"}, opts, true)
	if err != nil {
		t.Fatalf("Run() error: %v", err)
	}
	return results
}

// TestRunBinaryPresenceChangesNothing runs the same plan with a fake
// symskills binary on PATH and without it, asserting the planned targets
// are identical: the old binary can neither help nor break the sync.
func TestRunBinaryPresenceChangesNothing(t *testing.T) {
	t.Setenv("PATH", t.TempDir())
	withoutBinary := runBaseline(t)

	binDir := t.TempDir()
	t.Setenv("PATH", binDir)
	if err := os.WriteFile(filepath.Join(binDir, "symskills"), []byte("#!/bin/sh\nexit 1\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	withBinary := runBaseline(t)

	if !slices.Equal(withoutBinary, withBinary) {
		t.Errorf("results differ with binary present/absent:\nwithout: %+v\nwith:    %+v", withoutBinary, withBinary)
	}
}

// TestRun_UnsupportedHarness reports skipped rather than an error,
// mirroring the old bridge for harnesses the skills pipeline does not target.
func TestRun_UnsupportedHarness(t *testing.T) {
	opts := writeLibrary(t)
	results, err := Run(context.Background(), []string{"cursor", "claude-desktop"}, opts, false)
	if err != nil {
		t.Fatalf("Run() error: %v", err)
	}
	if len(results) != 2 {
		t.Fatalf("expected 2 results, got %d", len(results))
	}
	for _, r := range results {
		if r.Status != "skipped" {
			t.Errorf("%s: status %q, want skipped", r.Target, r.Status)
		}
	}
}

// TestRun_EmptyLibraryIsNotAnError matches the "no skills rendered" the
// bridge surfaced when symskills produced no output for a target.
func TestRun_EmptyLibraryIsNotAnError(t *testing.T) {
	opts := writeLibrary(t)
	results, err := Run(context.Background(), []string{"claude"}, opts, false)
	if err != nil {
		t.Fatalf("Run() error: %v", err)
	}
	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}
	if results[0].Status != "ok" {
		t.Errorf("empty library: status %q, want ok", results[0].Status)
	}
	if results[0].Message != "no skills rendered" {
		t.Errorf("empty library: message %q, want no skills rendered", results[0].Message)
	}
}

// TestRun_FailureIsVisibleAndReported covers acceptance criterion 3: a skill
// that cannot render surfaces as an error result naming the failing skill.
func TestRun_FailureIsVisibleAndReported(t *testing.T) {
	opts := writeLibrary(t)
	writeSkill(t, opts, "demo", validSkillMD)
	writeSkill(t, opts, "broken", invalidSkillMD)

	results, err := Run(context.Background(), []string{"claude"}, opts, false)
	if err != nil {
		t.Fatalf("Run() error: %v", err)
	}
	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}
	if results[0].Status != "error" {
		t.Fatalf("claude: status %q, want error", results[0].Status)
	}
	if !strings.Contains(results[0].Message, "broken") {
		t.Errorf("claude message %q should name the broken skill", results[0].Message)
	}
}

// TestRunTimeoutSurfacesAnError keeps the old per-target timeout semantics:
// a target past its budget is reported as an error, not silently dropped.
func TestRunTimeoutSurfacesAnError(t *testing.T) {
	opts := writeLibrary(t)
	opts.Timeout = 1 // ns — triggers the context immediately
	results, err := Run(context.Background(), []string{"claude"}, opts, false)
	if err != nil {
		t.Fatalf("Run() error: %v", err)
	}
	if len(results) != 1 {
		t.Fatalf("expected 1 result, got %d", len(results))
	}
	if results[0].Status != "error" {
		t.Errorf("status %q, want error", results[0].Status)
	}
}
