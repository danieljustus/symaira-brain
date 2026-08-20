//go:build !windows

// XDG Base Directory Specification tests — these use Unix path conventions
// (/tmp/…) and the XDG spec itself is POSIX-only. Excluded on Windows to
// avoid false failures from path separator differences. The cross-platform
// Default and Ensure tests in paths_test.go still run on Windows.
package paths

import (
	"testing"
)

func TestConfigDir_XDG(t *testing.T) {
	t.Setenv("XDG_CONFIG_HOME", "/tmp/test-xdg-config")

	dir, err := ConfigDir()
	if err != nil {
		t.Fatalf("ConfigDir() error: %v", err)
	}
	expected := "/tmp/test-xdg-config/symmemory"
	if dir != expected {
		t.Errorf("ConfigDir() = %q, want %q", dir, expected)
	}
}

func TestDataDir_XDG(t *testing.T) {
	t.Setenv("XDG_DATA_HOME", "/tmp/test-xdg-data")

	dir, err := DataDir()
	if err != nil {
		t.Fatalf("DataDir() error: %v", err)
	}
	expected := "/tmp/test-xdg-data/symmemory"
	if dir != expected {
		t.Errorf("DataDir() = %q, want %q", dir, expected)
	}
}

func TestSecretPath(t *testing.T) {
	t.Setenv("XDG_CONFIG_HOME", "/tmp/test-xdg-config")

	path, err := SecretPath("jwt.secret")
	if err != nil {
		t.Fatalf("SecretPath() error: %v", err)
	}
	expected := "/tmp/test-xdg-config/symmemory/jwt.secret"
	if path != expected {
		t.Errorf("SecretPath() = %q, want %q", path, expected)
	}
}

func TestSecretPath_CustomName(t *testing.T) {
	t.Setenv("XDG_CONFIG_HOME", "/tmp/test-xdg-config")

	path, err := SecretPath("api.key")
	if err != nil {
		t.Fatalf("SecretPath() error: %v", err)
	}
	expected := "/tmp/test-xdg-config/symmemory/api.key"
	if path != expected {
		t.Errorf("SecretPath() = %q, want %q", path, expected)
	}
}

func TestDatabasePath_XDG(t *testing.T) {
	t.Setenv("XDG_DATA_HOME", "/tmp/test-xdg-data")

	path, err := DatabasePath()
	if err != nil {
		t.Fatalf("DatabasePath() error: %v", err)
	}
	expected := "/tmp/test-xdg-data/symmemory/default.db"
	if path != expected {
		t.Errorf("DatabasePath() = %q, want %q", path, expected)
	}
}
