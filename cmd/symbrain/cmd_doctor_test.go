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

	for _, key := range []string{"config_dir", "data_dir", "cache_dir", "config", "servers", "profiles"} {
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
