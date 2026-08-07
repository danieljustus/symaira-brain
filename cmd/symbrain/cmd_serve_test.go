package main

import (
	"os"
	"path/filepath"
	"testing"
)

// withServeHome points $HOME at a fresh temp directory so profile.Load
// never touches the real user config.
func withServeHome(t *testing.T) string {
	t.Helper()
	home := t.TempDir()
	t.Setenv("HOME", home)
	return home
}

func TestResolveServeProfile_ByName(t *testing.T) {
	home := withServeHome(t)
	dir := filepath.Join(home, ".config", "symbrain", "profiles")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	path := filepath.Join(dir, "personal.toml")
	contents := "[profile]\nname = \"personal\"\n"
	if err := os.WriteFile(path, []byte(contents), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	p, err := resolveServeProfile("personal", "")
	if err != nil {
		t.Fatalf("resolveServeProfile() error = %v, want nil", err)
	}
	if p.Name != "personal" {
		t.Errorf("Name = %q, want %q", p.Name, "personal")
	}
}

func TestResolveServeProfile_ByFile(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "room.toml")
	contents := "[profile]\nname = \"room\"\n\n[servers.memory]\nenabled = true\nmode = \"read_only\"\n"
	if err := os.WriteFile(path, []byte(contents), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	p, err := resolveServeProfile("", path)
	if err != nil {
		t.Fatalf("resolveServeProfile() error = %v, want nil", err)
	}
	if p.Name != "room" {
		t.Errorf("Name = %q, want %q", p.Name, "room")
	}
	if !p.Servers.Memory.Enabled {
		t.Error("Memory should be enabled")
	}
}

func TestResolveServeProfile_Errors(t *testing.T) {
	t.Run("neither source", func(t *testing.T) {
		_, err := resolveServeProfile("", "")
		if err == nil {
			t.Fatal("resolveServeProfile() error = nil, want error")
		}
	})

	t.Run("both sources", func(t *testing.T) {
		_, err := resolveServeProfile("personal", "/tmp/room.toml")
		if err == nil {
			t.Fatal("resolveServeProfile() error = nil, want error")
		}
	})

	t.Run("missing file", func(t *testing.T) {
		_, err := resolveServeProfile("", filepath.Join(t.TempDir(), "nope.toml"))
		if err == nil {
			t.Fatal("resolveServeProfile() error = nil, want error for a missing file")
		}
	})
}
