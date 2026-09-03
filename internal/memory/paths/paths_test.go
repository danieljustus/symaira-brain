// Cross-platform path resolution tests — these work on both POSIX and Windows
// because they use filepath.Join for expected values or t.TempDir for temp dirs.
// XDG-specific tests with hardcoded Unix path conventions live in paths_unix_test.go
// and are guarded with //go:build !windows.
package paths

import (
	"os"
	"path/filepath"
	"testing"
)

func TestConfigDir_Default(t *testing.T) {
	// Isolate HOME too: a dev machine may have a legacy
	// ~/.config/symmemory install, which would make the "nothing exists
	// yet" default flake against that machine's state (#427).
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", "")

	dir, err := ConfigDir()
	if err != nil {
		t.Fatalf("ConfigDir() error: %v", err)
	}
	expected := filepath.Join(home, ".config", "symbrain", "memory")
	if dir != expected {
		t.Errorf("ConfigDir() = %q, want %q", dir, expected)
	}
}

func TestDataDir_Default(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_DATA_HOME", "")

	dir, err := DataDir()
	if err != nil {
		t.Fatalf("DataDir() error: %v", err)
	}
	expected := filepath.Join(home, ".local", "share", "symbrain", "memory")
	if dir != expected {
		t.Errorf("DataDir() = %q, want %q", dir, expected)
	}
}

func TestDatabasePath_Default(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_DATA_HOME", "")

	path, err := DatabasePath()
	if err != nil {
		t.Fatalf("DatabasePath() error: %v", err)
	}
	expected := filepath.Join(home, ".local", "share", "symbrain", "memory", "default.db")
	if path != expected {
		t.Errorf("DatabasePath() = %q, want %q", path, expected)
	}
}

func TestEnsureConfigDir(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_CONFIG_HOME", dir)

	result, err := EnsureConfigDir()
	if err != nil {
		t.Fatalf("EnsureConfigDir() error: %v", err)
	}

	info, err := os.Stat(result)
	if err != nil {
		t.Fatalf("EnsureConfigDir() did not create directory: %v", err)
	}
	if !info.IsDir() {
		t.Errorf("EnsureConfigDir() created %q, want a directory", result)
	}
}

func TestEnsureDataDir(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("XDG_DATA_HOME", dir)

	result, err := EnsureDataDir()
	if err != nil {
		t.Fatalf("EnsureDataDir() error: %v", err)
	}

	info, err := os.Stat(result)
	if err != nil {
		t.Fatalf("EnsureDataDir() did not create directory: %v", err)
	}
	if !info.IsDir() {
		t.Errorf("EnsureDataDir() created %q, want a directory", result)
	}
}

func TestConfigDir_FallsBackToLegacySymmemory(t *testing.T) {
	base := t.TempDir()
	t.Setenv("XDG_CONFIG_HOME", base)

	legacy := filepath.Join(base, "symmemory")
	if err := os.MkdirAll(legacy, 0o700); err != nil {
		t.Fatalf("MkdirAll(legacy) error: %v", err)
	}

	dir, err := ConfigDir()
	if err != nil {
		t.Fatalf("ConfigDir() error: %v", err)
	}
	if dir != legacy {
		t.Errorf("ConfigDir() = %q, want legacy %q", dir, legacy)
	}
}

func TestConfigLocation_ReportsLegacy(t *testing.T) {
	base := t.TempDir()
	t.Setenv("XDG_CONFIG_HOME", base)

	legacy := filepath.Join(base, "symmemory")
	if err := os.MkdirAll(legacy, 0o700); err != nil {
		t.Fatalf("MkdirAll(legacy) error: %v", err)
	}

	loc, err := ConfigLocation()
	if err != nil {
		t.Fatalf("ConfigLocation() error: %v", err)
	}
	if !loc.Legacy {
		t.Errorf("ConfigLocation().Legacy = false, want true when only the legacy dir exists")
	}
	if loc.Dir != legacy {
		t.Errorf("ConfigLocation().Dir = %q, want %q", loc.Dir, legacy)
	}
}

func TestDataLocation_ReportsCurrentWhenNoLegacy(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_DATA_HOME", "")

	loc, err := DataLocation()
	if err != nil {
		t.Fatalf("DataLocation() error: %v", err)
	}
	if loc.Legacy {
		t.Errorf("DataLocation().Legacy = true, want false when neither location exists")
	}
}

func TestSecretPath_UsesLegacyConfigDirWhenPresent(t *testing.T) {
	base := t.TempDir()
	t.Setenv("XDG_CONFIG_HOME", base)

	legacy := filepath.Join(base, "symmemory")
	if err := os.MkdirAll(legacy, 0o700); err != nil {
		t.Fatalf("MkdirAll(legacy) error: %v", err)
	}

	path, err := SecretPath("jwt.secret")
	if err != nil {
		t.Fatalf("SecretPath() error: %v", err)
	}
	expected := filepath.Join(legacy, "jwt.secret")
	if path != expected {
		t.Errorf("SecretPath() = %q, want %q", path, expected)
	}
}
