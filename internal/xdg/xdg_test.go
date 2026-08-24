package xdg

import (
	"path/filepath"
	"testing"
)

func TestConfigPath_UsesHomeConfigDir(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)

	got := ConfigPath()
	want := filepath.Join(home, ".config", "symbrain", "config.toml")
	if got != want {
		t.Errorf("ConfigPath() = %q, want %q", got, want)
	}
}

func TestConfigDir_IsParentOfConfigPath(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)

	if got, want := ConfigDir(), filepath.Join(home, ".config", "symbrain"); got != want {
		t.Errorf("ConfigDir() = %q, want %q", got, want)
	}
}

func TestProfilesDir_SitsUnderConfigDir(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)

	if got, want := ProfilesDir(), filepath.Join(home, ".config", "symbrain", "profiles"); got != want {
		t.Errorf("ProfilesDir() = %q, want %q", got, want)
	}
}

func TestDataDir_EnvOverride(t *testing.T) {
	base := t.TempDir()
	t.Setenv("XDG_DATA_HOME", base)

	got, err := DataDir()
	if err != nil {
		t.Fatalf("DataDir: %v", err)
	}
	if want := filepath.Join(base, "symbrain"); got != want {
		t.Errorf("DataDir() = %q, want %q", got, want)
	}
}

func TestDataDir_FallbackToHome(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_DATA_HOME", "")

	got, err := DataDir()
	if err != nil {
		t.Fatalf("DataDir: %v", err)
	}
	if want := filepath.Join(home, ".local", "share", "symbrain"); got != want {
		t.Errorf("DataDir() = %q, want %q", got, want)
	}
}

func TestAuditDir_SitsUnderDataDir(t *testing.T) {
	base := t.TempDir()
	t.Setenv("XDG_DATA_HOME", base)
	got, err := AuditDir()
	if err != nil {
		t.Fatalf("AuditDir: %v", err)
	}
	want := filepath.Join(base, "symbrain", "audit")
	if got != want {
		t.Errorf("AuditDir() = %q, want %q", got, want)
	}
}

func TestPatternsDir_SitsUnderDataDir(t *testing.T) {
	base := t.TempDir()
	t.Setenv("XDG_DATA_HOME", base)
	got, err := PatternsDir()
	if err != nil {
		t.Fatalf("PatternsDir: %v", err)
	}
	want := filepath.Join(base, "symbrain", "recipes")
	if got != want {
		t.Errorf("PatternsDir() = %q, want %q", got, want)
	}
}

func TestCacheDir_EnvOverride(t *testing.T) {
	base := t.TempDir()
	t.Setenv("XDG_CACHE_HOME", base)

	got, err := CacheDir()
	if err != nil {
		t.Fatalf("CacheDir: %v", err)
	}
	if want := filepath.Join(base, "symbrain"); got != want {
		t.Errorf("CacheDir() = %q, want %q", got, want)
	}
}

func TestCacheDir_FallbackToHome(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CACHE_HOME", "")
	got, err := CacheDir()
	if err != nil {
		t.Fatalf("CacheDir: %v", err)
	}
	if want := filepath.Join(home, ".cache", "symbrain"); got != want {
		t.Errorf("CacheDir() = %q, want %q", got, want)
	}
}

func TestManagedBinDir(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	got, err := ManagedBinDir()
	if err != nil {
		t.Fatalf("ManagedBinDir: %v", err)
	}
	want := filepath.Join(home, ".symaira", "bin")
	if got != want {
		t.Errorf("ManagedBinDir() = %q, want %q", got, want)
	}
}
