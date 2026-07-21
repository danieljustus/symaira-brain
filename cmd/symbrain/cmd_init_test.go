package main

import (
	"bytes"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"

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

	if strings.Count(stdout.String(), "created ") != 3 {
		t.Errorf("stdout = %q, want 3 'created' lines", stdout.String())
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
	if strings.Count(stdout.String(), "already exists") != 3 {
		t.Errorf("second run stdout = %q, want 3 'already exists' lines", stdout.String())
	}
}
