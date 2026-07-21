package main

import (
	"bytes"
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// isolatedPATH points $PATH at dir only, so exec.LookPath cannot find any
// real binary installed on the machine running the test.
func isolatedPATH(t *testing.T, dir string) {
	t.Helper()
	t.Setenv("PATH", dir)
}

// fakeBinary writes an executable shell script named name into dir that
// prints stdout on `version --json` args and exits 0, or exits 1 with no
// output for any other args.
func fakeBinary(t *testing.T, dir, name, stdout string) string {
	t.Helper()
	path := filepath.Join(dir, name)
	script := "#!/bin/sh\n" +
		`if [ "$1" = "version" ] && [ "$2" = "--json" ]; then` + "\n" +
		"  printf '%s'\n" +
		"  exit 0\n" +
		"fi\n" +
		"exit 1\n"
	script = strings.Replace(script, "%s", stdout, 1)
	if err := os.WriteFile(path, []byte(script), 0o755); err != nil {
		t.Fatalf("write fake binary %s: %v", name, err)
	}
	return path
}

func TestCheckServers_NotFound(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("fake binary is a POSIX shell script")
	}
	isolatedPATH(t, t.TempDir())

	checks := checkServers(context.Background())

	if len(checks) != len(knownServers) {
		t.Fatalf("len(checks) = %d, want %d", len(checks), len(knownServers))
	}
	for _, c := range checks {
		if c.Found {
			t.Errorf("server %s: Found = true, want false (isolated PATH)", c.Name)
		}
		if c.InstallHint == "" {
			t.Errorf("server %s: InstallHint is empty, want an install hint", c.Name)
		}
	}
}

func TestCheckServers_FoundAndProbeSucceeds(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("fake binary is a POSIX shell script")
	}
	dir := t.TempDir()
	fakeBinary(t, dir, "symvault", `{"tool":"symvault","version":"9.9.9","schema_version":1}`)
	isolatedPATH(t, dir)

	checks := checkServers(context.Background())

	var vault *serverCheck
	for i := range checks {
		if checks[i].Name == "vault" {
			vault = &checks[i]
		}
	}
	if vault == nil {
		t.Fatal("no check for server \"vault\"")
	}
	if !vault.Found {
		t.Fatal("vault.Found = false, want true")
	}
	if vault.ProbeError != "" {
		t.Errorf("vault.ProbeError = %q, want empty", vault.ProbeError)
	}
	if vault.Version != "9.9.9" {
		t.Errorf("vault.Version = %q, want %q", vault.Version, "9.9.9")
	}
}

func TestCheckServers_FoundButProbeFails(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("fake binary is a POSIX shell script")
	}
	dir := t.TempDir()
	fakeBinary(t, dir, "symmemory", "not json")
	isolatedPATH(t, dir)

	checks := checkServers(context.Background())

	var memory *serverCheck
	for i := range checks {
		if checks[i].Name == "memory" {
			memory = &checks[i]
		}
	}
	if memory == nil {
		t.Fatal("no check for server \"memory\"")
	}
	if !memory.Found {
		t.Fatal("memory.Found = false, want true (binary is on PATH)")
	}
	if memory.ProbeError == "" {
		t.Error("memory.ProbeError is empty, want a parse error (output is not JSON)")
	}
}

func TestCheckDir(t *testing.T) {
	dir := t.TempDir()
	missing := filepath.Join(dir, "does-not-exist")

	if c := checkDir(dir); !c.Exists {
		t.Errorf("checkDir(%q).Exists = false, want true", dir)
	}
	if c := checkDir(missing); c.Exists {
		t.Errorf("checkDir(%q).Exists = true, want false", missing)
	}
}

func TestDiscoverProfiles(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)

	if got := discoverProfiles(); len(got) != 0 {
		t.Errorf("discoverProfiles() with no profiles dir = %v, want empty", got)
	}

	profilesDir := filepath.Join(home, ".config", "symbrain", "profiles")
	if err := os.MkdirAll(profilesDir, 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	for _, name := range []string{"restricted.toml", "personal.toml", "notes.txt"} {
		if err := os.WriteFile(filepath.Join(profilesDir, name), []byte("x"), 0o644); err != nil {
			t.Fatalf("WriteFile(%s): %v", name, err)
		}
	}

	got := discoverProfiles()
	want := []string{"personal", "restricted"}
	if len(got) != len(want) || got[0] != want[0] || got[1] != want[1] {
		t.Errorf("discoverProfiles() = %v, want %v", got, want)
	}
}

func TestCmdDoctor_JSONIsSnakeCaseAndExitsOK(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("fake binary is a POSIX shell script")
	}
	home := t.TempDir()
	t.Setenv("HOME", home)
	isolatedPATH(t, t.TempDir())

	var stdout, stderr bytes.Buffer
	code := cmdDoctor([]string{"--json"}, &stdout, &stderr)

	if code != exitcodes.ExitOK {
		t.Fatalf("cmdDoctor(--json) = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	var report map[string]any
	if err := json.Unmarshal(stdout.Bytes(), &report); err != nil {
		t.Fatalf("output is not valid JSON: %v (output: %q)", err, stdout.String())
	}

	for _, key := range []string{"config_dir", "data_dir", "cache_dir", "config", "servers", "profiles", "harnesses"} {
		if _, ok := report[key]; !ok {
			t.Errorf("JSON report missing key %q: %v", key, report)
		}
	}
}

func TestCmdDoctor_ExitsOKEvenWithNoOptionalToolsInstalled(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("fake binary is a POSIX shell script")
	}
	home := t.TempDir()
	t.Setenv("HOME", home)
	isolatedPATH(t, t.TempDir())

	var stdout, stderr bytes.Buffer
	code := cmdDoctor(nil, &stdout, &stderr)

	if code != exitcodes.ExitOK {
		t.Fatalf("cmdDoctor() = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	if !strings.Contains(stdout.String(), "not found on PATH") {
		t.Errorf("stdout = %q, want it to explain missing optional tools", stdout.String())
	}
}

// writeHarnessConfig writes a config file for h at its resolved config
// path, with symbrain already installed and bound to profile. It uses the
// harness package's own Document/Marshal so the fixture is guaranteed to
// be something checkHarness can parse (issue #21 builds directly on #19's
// registry).
func writeHarnessConfig(t *testing.T, h harness.Harness, profile string) string {
	t.Helper()
	path, err := h.ConfigPath()
	if err != nil {
		t.Fatalf("ConfigPath: %v", err)
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}

	doc := harness.Empty(h)
	doc.SetServer(harness.ServerName, harness.NewEntry(profile))
	data, err := doc.Marshal()
	if err != nil {
		t.Fatalf("Marshal: %v", err)
	}
	if err := os.WriteFile(path, data, 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}
	return path
}

// writeProfileFile creates an (empty-content) profile file at
// ~/.config/symbrain/profiles/<name>.toml, the on-disk existence
// checkHarness looks for per issue #21's narrower "no internal/profile
// yet" fallback.
func writeProfileFile(t *testing.T, home, name string) {
	t.Helper()
	dir := filepath.Join(home, ".config", "symbrain", "profiles")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	content := "[profile]\nname = \"" + name + "\"\n"
	if err := os.WriteFile(filepath.Join(dir, name+".toml"), []byte(content), 0o644); err != nil {
		t.Fatalf("WriteFile profile: %v", err)
	}
}

func TestCheckHarness_NoConfig(t *testing.T) {
	harnessSandbox(t)
	h := harnessByName(t, "claude")

	check := checkHarness(h)
	if check.ConfigFound {
		t.Error("ConfigFound = true, want false")
	}
	if check.Installed {
		t.Error("Installed = true, want false")
	}
	if check.ConfigPath == "" {
		t.Error("ConfigPath is empty, want the resolved path even when the file is missing")
	}
}

func TestCheckHarness_ConfigFoundButNotInstalled(t *testing.T) {
	harnessSandbox(t)
	h := harnessByName(t, "cursor")
	path, err := h.ConfigPath()
	if err != nil {
		t.Fatalf("ConfigPath: %v", err)
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	if err := os.WriteFile(path, []byte("{\n  \"mcpServers\": {}\n}\n"), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	check := checkHarness(h)
	if !check.ConfigFound || !check.ConfigParsed {
		t.Fatalf("ConfigFound=%v ConfigParsed=%v, want true/true", check.ConfigFound, check.ConfigParsed)
	}
	if check.Installed {
		t.Error("Installed = true, want false")
	}
}

func TestCheckHarness_InstalledAndProfileExists(t *testing.T) {
	home := harnessSandbox(t)
	h := harnessByName(t, "claude")
	writeHarnessConfig(t, h, "personal")
	writeProfileFile(t, home, "personal")

	check := checkHarness(h)
	if !check.Installed {
		t.Fatal("Installed = false, want true")
	}
	if check.Profile != "personal" {
		t.Errorf("Profile = %q, want %q", check.Profile, "personal")
	}
	if !check.ProfileExists {
		t.Error("ProfileExists = false, want true")
	}
	if check.ProfileMissing {
		t.Error("ProfileMissing = true, want false")
	}
}

func TestCheckHarness_InstalledButProfileMissing(t *testing.T) {
	home := harnessSandbox(t)
	h := harnessByName(t, "claude")
	writeHarnessConfig(t, h, "ghost")
	_ = home // deliberately do not create profiles/ghost.toml

	check := checkHarness(h)
	if !check.Installed {
		t.Fatal("Installed = false, want true")
	}
	if check.ProfileExists {
		t.Error("ProfileExists = true, want false")
	}
	if !check.ProfileMissing {
		t.Error("ProfileMissing = false, want true — a binding to a nonexistent profile must be flagged")
	}
}

func TestCheckHarness_CorruptConfig(t *testing.T) {
	harnessSandbox(t)
	h := harnessByName(t, "claude")
	path, err := h.ConfigPath()
	if err != nil {
		t.Fatalf("ConfigPath: %v", err)
	}
	if err := os.WriteFile(path, []byte("{not valid json"), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	check := checkHarness(h)
	if !check.ConfigFound {
		t.Error("ConfigFound = false, want true")
	}
	if check.ConfigParsed {
		t.Error("ConfigParsed = true, want false")
	}
	if check.ConfigError == "" {
		t.Error("ConfigError is empty, want an error message")
	}
	if check.Installed {
		t.Error("Installed = true, want false for a corrupt config")
	}
}

func TestCheckHarness_ForeignEntrySharingTheNameIsNotInstalled(t *testing.T) {
	harnessSandbox(t)
	h := harnessByName(t, "cursor")
	path, err := h.ConfigPath()
	if err != nil {
		t.Fatalf("ConfigPath: %v", err)
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	doc := harness.Empty(h)
	doc.SetServer(harness.ServerName, harness.Entry{Command: "not-symbrain", Args: []string{"foo"}})
	data, err := doc.Marshal()
	if err != nil {
		t.Fatalf("Marshal: %v", err)
	}
	if err := os.WriteFile(path, data, 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	check := checkHarness(h)
	if check.Installed {
		t.Error("Installed = true, want false — command does not resolve to symbrain")
	}
}

func TestCheckHarnesses_CoversEveryRegisteredHarness(t *testing.T) {
	harnessSandbox(t)
	checks := checkHarnesses()
	if len(checks) != len(harness.All) {
		t.Fatalf("len(checkHarnesses()) = %d, want %d", len(checks), len(harness.All))
	}
}

func TestCmdDoctor_HumanOutput_ReportsHarnessBindings(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("fake binary is a POSIX shell script")
	}
	home := harnessSandbox(t)
	isolatedPATH(t, t.TempDir())

	installed := harnessByName(t, "cursor")
	writeHarnessConfig(t, installed, "personal")
	writeProfileFile(t, home, "personal")

	missing := harnessByName(t, "gemini")
	writeHarnessConfig(t, missing, "ghost")

	var stdout, stderr bytes.Buffer
	code := cmdDoctor(nil, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("cmdDoctor() = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	out := stdout.String()
	if !strings.Contains(out, "cursor") || !strings.Contains(out, `"personal"`) {
		t.Errorf("stdout missing the installed cursor/personal line: %q", out)
	}
	if !strings.Contains(out, `missing profile "ghost"`) {
		t.Errorf("stdout missing the flagged missing-profile line: %q", out)
	}
	if !strings.Contains(out, "codex") {
		t.Errorf("stdout missing a harness with no config at all: %q", out)
	}
}

func TestCmdDoctor_JSON_HarnessesIncludeProfileBindingFields(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("fake binary is a POSIX shell script")
	}
	home := harnessSandbox(t)
	isolatedPATH(t, t.TempDir())

	h := harnessByName(t, "codex")
	writeHarnessConfig(t, h, "personal")
	writeProfileFile(t, home, "personal")

	var stdout, stderr bytes.Buffer
	code := cmdDoctor([]string{"--json"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("cmdDoctor(--json) = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	type harnessJSON struct {
		Name           string `json:"name"`
		Installed      bool   `json:"installed"`
		Profile        string `json:"profile"`
		ProfileExists  bool   `json:"profile_exists"`
		ProfileMissing bool   `json:"profile_missing"`
	}
	var report struct {
		Harnesses []harnessJSON `json:"harnesses"`
	}
	if err := json.Unmarshal(stdout.Bytes(), &report); err != nil {
		t.Fatalf("output is not valid JSON: %v (output: %q)", err, stdout.String())
	}

	var codex *harnessJSON
	for i := range report.Harnesses {
		if report.Harnesses[i].Name == "codex" {
			codex = &report.Harnesses[i]
		}
	}
	if codex == nil {
		t.Fatalf("no codex entry in harnesses: %+v", report.Harnesses)
	}
	if !codex.Installed || codex.Profile != "personal" || !codex.ProfileExists || codex.ProfileMissing {
		t.Errorf("codex check = %+v, want installed/personal/exists/not-missing", *codex)
	}
}

func TestRunDoctorChecks_FlagsMissingProfileBinding(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("fake binary is a POSIX shell script")
	}
	harnessSandbox(t)
	isolatedPATH(t, t.TempDir())

	h := harnessByName(t, "gemini")
	writeHarnessConfig(t, h, "ghost")

	report := runDoctorChecks(context.Background())

	var found *harnessCheck
	for i := range report.Harnesses {
		if report.Harnesses[i].Name == "gemini" {
			found = &report.Harnesses[i]
		}
	}
	if found == nil {
		t.Fatal("no harness check for gemini")
	}
	if !found.ProfileMissing {
		t.Error("ProfileMissing = false, want true")
	}
}
