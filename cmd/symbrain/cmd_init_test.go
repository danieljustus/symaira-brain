package main

import (
	"bytes"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/profile"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func sandboxHome(t *testing.T) string {
	t.Helper()
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_DATA_HOME", "")
	t.Setenv("XDG_CACHE_HOME", "")
	return home
}

func TestCmdInit_FreshRunCreatesConfigAndProfiles(t *testing.T) {
	home := sandboxHome(t)

	var stdout, stderr bytes.Buffer
	code := cmdInit(nil, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("cmdInit() = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	for _, p := range []string{
		filepath.Join(home, ".config", "symbrain", "config.toml"),
		filepath.Join(home, ".config", "symbrain", "profiles", "personal.toml"),
		filepath.Join(home, ".config", "symbrain", "profiles", "restricted.toml"),
		filepath.Join(home, ".config", "symbrain", "profiles", "foreign-read-only.toml"),
	} {
		if _, err := os.Stat(p); err != nil {
			t.Errorf("expected %s to exist: %v", p, err)
		}
	}

	for _, dir := range []string{
		filepath.Join(home, ".local", "share", "symbrain", "audit"),
		filepath.Join(home, ".cache", "symbrain"),
	} {
		info, err := os.Stat(dir)
		if err != nil || !info.IsDir() {
			t.Errorf("expected directory %s to exist: %v", dir, err)
		}
	}

	if strings.Count(stdout.String(), "created ") != 4 {
		t.Errorf("stdout = %q, want 4 'created' lines", stdout.String())
	}
}

func TestCmdInit_SecondRunIsIdempotent(t *testing.T) {
	home := sandboxHome(t)

	if code := cmdInit(nil, io.Discard, io.Discard); code != exitcodes.ExitOK {
		t.Fatalf("first run: cmdInit() = %d", code)
	}

	configPath := filepath.Join(home, ".config", "symbrain", "config.toml")
	before, err := os.ReadFile(configPath)
	if err != nil {
		t.Fatalf("read config after first run: %v", err)
	}

	var stdout bytes.Buffer
	if code := cmdInit(nil, &stdout, io.Discard); code != exitcodes.ExitOK {
		t.Fatalf("second run: cmdInit() = %d", code)
	}

	after, err := os.ReadFile(configPath)
	if err != nil {
		t.Fatalf("read config after second run: %v", err)
	}
	if string(before) != string(after) {
		t.Error("second run modified an already-existing config file")
	}

	if strings.Contains(stdout.String(), "created ") {
		t.Errorf("second run should not create anything: %q", stdout.String())
	}
	if strings.Count(stdout.String(), "already exists") != 4 {
		t.Errorf("second run stdout = %q, want 4 'already exists' lines", stdout.String())
	}
}

// TestForeignReadOnlyProfile_LoadsWithoutWarnings proves the shipped
// foreign-server example is not just syntactically valid TOML but a
// profile the loader accepts cleanly — the acceptance criterion for
// issue #444 is that a user has a working example to copy, not merely
// one that parses.
func TestForeignReadOnlyProfile_LoadsWithoutWarnings(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "foreign-read-only.toml")
	if err := os.WriteFile(path, []byte(foreignReadOnlyProfileTOML), 0o644); err != nil {
		t.Fatalf("write profile: %v", err)
	}

	p, err := profile.LoadFile(path)
	if err != nil {
		t.Fatalf("LoadFile: %v", err)
	}
	if len(p.Warnings) != 0 {
		t.Errorf("Warnings = %v, want none", p.Warnings)
	}

	docs, ok := p.Servers["docs"]
	if !ok {
		t.Fatal("expected a foreign \"docs\" server in the example profile")
	}
	if docs.Access != profile.ForeignAccessRead {
		t.Errorf("docs.Access = %q, want %q", docs.Access, profile.ForeignAccessRead)
	}
	if len(docs.ToolsRead) == 0 {
		t.Error("docs.ToolsRead is empty — the example must show the tools_read fallback path, not just prose")
	}
}
