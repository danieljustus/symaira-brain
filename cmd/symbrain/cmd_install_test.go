package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// harnessSandbox is sandboxHome (see cmd_init_test.go) plus a cleared
// XDG_CONFIG_HOME, so every harness.ConfigPath() resolves entirely inside
// the sandbox regardless of what the host environment has set.
func harnessSandbox(t *testing.T) string {
	t.Helper()
	home := sandboxHome(t)
	t.Setenv("XDG_CONFIG_HOME", "")
	return home
}

func writeGlobalConfig(t *testing.T, home, defaultProfile string) {
	t.Helper()
	dir := filepath.Join(home, ".config", "symbrain")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	content := "default_profile = \"" + defaultProfile + "\"\n"
	if err := os.WriteFile(filepath.Join(dir, "config.toml"), []byte(content), 0o644); err != nil {
		t.Fatalf("WriteFile config.toml: %v", err)
	}
}

func TestCmdInstall_MissingHarnessFlag(t *testing.T) {
	harnessSandbox(t)
	var stdout, stderr bytes.Buffer
	code := cmdInstall([]string{"--profile", "personal"}, &stdout, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("code = %d, want %d", code, exitcodes.ExitNoInput)
	}
	if !strings.Contains(stderr.String(), "--harness is required") {
		t.Errorf("stderr = %q, want it to mention --harness is required", stderr.String())
	}
}

func TestCmdInstall_UnknownHarness(t *testing.T) {
	harnessSandbox(t)
	var stdout, stderr bytes.Buffer
	code := cmdInstall([]string{"--harness", "nonexistent", "--profile", "personal"}, &stdout, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitNoInput, stderr.String())
	}
}

func TestCmdInstall_NoProfileAndNoDefault(t *testing.T) {
	home := harnessSandbox(t)
	writeGlobalConfig(t, home, "")

	var stdout, stderr bytes.Buffer
	code := cmdInstall([]string{"--harness", "claude"}, &stdout, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitNoInput, stderr.String())
	}
	if !strings.Contains(stderr.String(), "no default_profile") {
		t.Errorf("stderr = %q, want it to explain the missing profile", stderr.String())
	}
}

func TestCmdInstall_UsesDefaultProfileFromGlobalConfig(t *testing.T) {
	home := harnessSandbox(t)
	writeGlobalConfig(t, home, "personal")

	var stdout, stderr bytes.Buffer
	code := cmdInstall([]string{"--harness", "claude"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	path := filepath.Join(home, ".claude.json")
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	doc, err := harness.Parse(harnessByName(t, "claude"), data)
	if err != nil {
		t.Fatalf("harness.Parse: %v", err)
	}
	entry, ok := doc.Server(harness.ServerName)
	if !ok {
		t.Fatal("symbrain entry not present after install")
	}
	profile, ok := entry.Profile()
	if !ok || profile != "personal" {
		t.Errorf("bound profile = %q, ok=%v, want %q", profile, ok, "personal")
	}
}

func TestCmdInstall_ExplicitProfileFlagWinsOverDefault(t *testing.T) {
	home := harnessSandbox(t)
	writeGlobalConfig(t, home, "personal")

	var stdout, stderr bytes.Buffer
	code := cmdInstall([]string{"--harness", "claude", "--profile", "restricted"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	path := filepath.Join(home, ".claude.json")
	data, _ := os.ReadFile(path)
	doc, _ := harness.Parse(harnessByName(t, "claude"), data)
	entry, _ := doc.Server(harness.ServerName)
	if profile, _ := entry.Profile(); profile != "restricted" {
		t.Errorf("bound profile = %q, want %q", profile, "restricted")
	}
}

func TestCmdInstall_BacksUpExistingConfig(t *testing.T) {
	home := harnessSandbox(t)
	path := filepath.Join(home, ".cursor", "mcp.json")
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	original := []byte("{\n  \"unrelated\": true\n}\n")
	if err := os.WriteFile(path, original, 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	var stdout, stderr bytes.Buffer
	code := cmdInstall([]string{"--harness", "cursor", "--profile", "personal"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	matches, err := filepath.Glob(path + ".bak.*")
	if err != nil {
		t.Fatalf("Glob: %v", err)
	}
	if len(matches) != 1 {
		t.Fatalf("backups = %v, want exactly 1", matches)
	}
	backupContent, err := os.ReadFile(matches[0])
	if err != nil {
		t.Fatalf("ReadFile(backup): %v", err)
	}
	if string(backupContent) != string(original) {
		t.Errorf("backup content = %q, want %q", backupContent, original)
	}
	if !strings.Contains(stdout.String(), "backed up") {
		t.Errorf("stdout = %q, want it to mention the backup", stdout.String())
	}
}

func TestCmdInstall_CreatesNewConfigWhenNoneExists(t *testing.T) {
	home := harnessSandbox(t)
	path := filepath.Join(home, ".gemini", "settings.json")

	var stdout, stderr bytes.Buffer
	code := cmdInstall([]string{"--harness", "gemini", "--profile", "personal"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	if _, err := os.Stat(path); err != nil {
		t.Fatalf("expected %s to be created: %v", path, err)
	}
	// No backup should exist: there was nothing to back up.
	matches, _ := filepath.Glob(path + ".bak.*")
	if len(matches) != 0 {
		t.Errorf("unexpected backups for a brand-new file: %v", matches)
	}
}

func TestCmdInstall_DryRun_WritesNothing(t *testing.T) {
	home := harnessSandbox(t)
	path := filepath.Join(home, ".cursor", "mcp.json")
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	original := []byte("{\n  \"unrelated\": true\n}\n")
	if err := os.WriteFile(path, original, 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}
	before, err := os.Stat(path)
	if err != nil {
		t.Fatalf("Stat: %v", err)
	}

	var stdout, stderr bytes.Buffer
	code := cmdInstall([]string{"--harness", "cursor", "--profile", "personal", "--dry-run"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	after, err := os.Stat(path)
	if err != nil {
		t.Fatalf("Stat after dry-run: %v", err)
	}
	if !before.ModTime().Equal(after.ModTime()) {
		t.Errorf("mtime changed: before %v, after %v", before.ModTime(), after.ModTime())
	}
	content, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	if string(content) != string(original) {
		t.Errorf("content changed by --dry-run:\ngot:  %q\nwant: %q", content, original)
	}

	matches, _ := filepath.Glob(path + ".bak.*")
	if len(matches) != 0 {
		t.Errorf("--dry-run created a backup: %v", matches)
	}

	if !strings.Contains(stdout.String(), "+") {
		t.Errorf("dry-run stdout should contain a diff with added lines: %q", stdout.String())
	}
	if !strings.Contains(stdout.String(), "symbrain") {
		t.Errorf("dry-run diff should mention the symbrain entry: %q", stdout.String())
	}
}

func TestCmdInstall_RefusesCorruptConfig(t *testing.T) {
	home := harnessSandbox(t)
	path := filepath.Join(home, ".claude.json")
	if err := os.WriteFile(path, []byte("{not valid json"), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}
	before, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}

	var stdout, stderr bytes.Buffer
	code := cmdInstall([]string{"--harness", "claude", "--profile", "personal"}, &stdout, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("code = %d, want %d (ExitNoInput) (stderr: %s)", code, exitcodes.ExitNoInput, stderr.String())
	}

	after, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("ReadFile after refusal: %v", err)
	}
	if string(after) != string(before) {
		t.Error("install modified a config file it should have refused to parse")
	}
	matches, _ := filepath.Glob(path + ".bak.*")
	if len(matches) != 0 {
		t.Errorf("install backed up a config it refused to edit: %v", matches)
	}
}

func TestCmdInstall_ProjectFlag_UnsupportedHarness(t *testing.T) {
	harnessSandbox(t)
	var stdout, stderr bytes.Buffer
	code := cmdInstall([]string{"--harness", "cursor", "--profile", "personal", "--project", "/tmp/some-project"}, &stdout, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitNoInput, stderr.String())
	}
	if !strings.Contains(stderr.String(), "no project-local config") {
		t.Errorf("stderr = %q, want it to explain cursor has no project config", stderr.String())
	}
}

func TestCmdInstall_ProjectFlag_Claude(t *testing.T) {
	harnessSandbox(t)
	projectDir := t.TempDir()

	var stdout, stderr bytes.Buffer
	code := cmdInstall([]string{"--harness", "claude", "--profile", "personal", "--project", projectDir}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	path := filepath.Join(projectDir, ".mcp.json")
	if _, err := os.Stat(path); err != nil {
		t.Fatalf("expected project-local config at %s: %v", path, err)
	}
}

func harnessByName(t *testing.T, name string) harness.Harness {
	t.Helper()
	h, err := harness.Lookup(name)
	if err != nil {
		t.Fatalf("harness.Lookup(%q): %v", name, err)
	}
	return h
}
