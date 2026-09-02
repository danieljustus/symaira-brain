package paths

import (
	"os"
	"path/filepath"
	"testing"
)

func TestMemoryConfigDir_DefaultsToCurrentNamespace(t *testing.T) {
	// Isolate HOME too: a real dev machine may have a legacy
	// ~/.config/symmemory from before this package existed, which would
	// make this "neither exists" case flake against that machine's state.
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", "")

	loc, err := MemoryConfigDir()
	if err != nil {
		t.Fatalf("MemoryConfigDir() error: %v", err)
	}
	want := filepath.Join(home, ".config", "symbrain", "memory")
	if loc.Dir != want {
		t.Errorf("MemoryConfigDir() = %q, want %q", loc.Dir, want)
	}
	if loc.Legacy {
		t.Errorf("MemoryConfigDir() Legacy = true, want false when neither location exists")
	}
}

func TestMemoryConfigDir_HonorsXDGEnv(t *testing.T) {
	base := t.TempDir()
	t.Setenv("XDG_CONFIG_HOME", base)

	loc, err := MemoryConfigDir()
	if err != nil {
		t.Fatalf("MemoryConfigDir() error: %v", err)
	}
	want := filepath.Join(base, "symbrain", "memory")
	if loc.Dir != want {
		t.Errorf("MemoryConfigDir() = %q, want %q", loc.Dir, want)
	}
	if loc.Legacy {
		t.Errorf("MemoryConfigDir() Legacy = true, want false")
	}
}

func TestMemoryConfigDir_FallsBackToLegacyWhenOnlyLegacyExists(t *testing.T) {
	base := t.TempDir()
	t.Setenv("XDG_CONFIG_HOME", base)

	legacy := filepath.Join(base, "symmemory")
	if err := os.MkdirAll(legacy, 0o700); err != nil {
		t.Fatalf("MkdirAll(legacy) error: %v", err)
	}

	loc, err := MemoryConfigDir()
	if err != nil {
		t.Fatalf("MemoryConfigDir() error: %v", err)
	}
	if loc.Dir != legacy {
		t.Errorf("MemoryConfigDir() = %q, want legacy %q", loc.Dir, legacy)
	}
	if !loc.Legacy {
		t.Errorf("MemoryConfigDir() Legacy = false, want true when only the legacy dir exists")
	}
}

func TestMemoryConfigDir_PrefersCurrentWhenBothExist(t *testing.T) {
	base := t.TempDir()
	t.Setenv("XDG_CONFIG_HOME", base)

	legacy := filepath.Join(base, "symmemory")
	current := filepath.Join(base, "symbrain", "memory")
	if err := os.MkdirAll(legacy, 0o700); err != nil {
		t.Fatalf("MkdirAll(legacy) error: %v", err)
	}
	if err := os.MkdirAll(current, 0o700); err != nil {
		t.Fatalf("MkdirAll(current) error: %v", err)
	}

	loc, err := MemoryConfigDir()
	if err != nil {
		t.Fatalf("MemoryConfigDir() error: %v", err)
	}
	if loc.Dir != current {
		t.Errorf("MemoryConfigDir() = %q, want current %q", loc.Dir, current)
	}
	if loc.Legacy {
		t.Errorf("MemoryConfigDir() Legacy = true, want false when the current dir exists")
	}
}

func TestMemoryDataDir(t *testing.T) {
	base := t.TempDir()
	t.Setenv("XDG_DATA_HOME", base)

	loc, err := MemoryDataDir()
	if err != nil {
		t.Fatalf("MemoryDataDir() error: %v", err)
	}
	want := filepath.Join(base, "symbrain", "memory")
	if loc.Dir != want {
		t.Errorf("MemoryDataDir() = %q, want %q", loc.Dir, want)
	}
}

func TestSkillsConfigDir_HonorsXDGEnv(t *testing.T) {
	// Legacy symskills ignored XDG_CONFIG_HOME entirely; the shared
	// resolver must honor it for skills just as it does for memory.
	base := t.TempDir()
	t.Setenv("XDG_CONFIG_HOME", base)

	loc, err := SkillsConfigDir()
	if err != nil {
		t.Fatalf("SkillsConfigDir() error: %v", err)
	}
	want := filepath.Join(base, "symbrain", "skills")
	if loc.Dir != want {
		t.Errorf("SkillsConfigDir() = %q, want %q", loc.Dir, want)
	}
}

func TestSkillsConfigDir_FallsBackToLegacy(t *testing.T) {
	base := t.TempDir()
	t.Setenv("XDG_CONFIG_HOME", base)

	legacy := filepath.Join(base, "symskills")
	if err := os.MkdirAll(legacy, 0o700); err != nil {
		t.Fatalf("MkdirAll(legacy) error: %v", err)
	}

	loc, err := SkillsConfigDir()
	if err != nil {
		t.Fatalf("SkillsConfigDir() error: %v", err)
	}
	if loc.Dir != legacy {
		t.Errorf("SkillsConfigDir() = %q, want legacy %q", loc.Dir, legacy)
	}
	if !loc.Legacy {
		t.Errorf("SkillsConfigDir() Legacy = false, want true")
	}
}

func TestSkillsDataDir_HonorsXDGEnv(t *testing.T) {
	base := t.TempDir()
	t.Setenv("XDG_DATA_HOME", base)

	loc, err := SkillsDataDir()
	if err != nil {
		t.Fatalf("SkillsDataDir() error: %v", err)
	}
	want := filepath.Join(base, "symbrain", "skills")
	if loc.Dir != want {
		t.Errorf("SkillsDataDir() = %q, want %q", loc.Dir, want)
	}
}

func TestSkillsCacheDir_HonorsXDGEnv(t *testing.T) {
	// Legacy symskills ignored XDG_CACHE_HOME entirely (issue #427).
	base := t.TempDir()
	t.Setenv("XDG_CACHE_HOME", base)

	loc, err := SkillsCacheDir()
	if err != nil {
		t.Fatalf("SkillsCacheDir() error: %v", err)
	}
	want := filepath.Join(base, "symbrain", "skills")
	if loc.Dir != want {
		t.Errorf("SkillsCacheDir() = %q, want %q", loc.Dir, want)
	}
}

func TestSkillsCacheDir_FallsBackToLegacy(t *testing.T) {
	base := t.TempDir()
	t.Setenv("XDG_CACHE_HOME", base)

	legacy := filepath.Join(base, "symskills")
	if err := os.MkdirAll(legacy, 0o700); err != nil {
		t.Fatalf("MkdirAll(legacy) error: %v", err)
	}

	loc, err := SkillsCacheDir()
	if err != nil {
		t.Fatalf("SkillsCacheDir() error: %v", err)
	}
	if loc.Dir != legacy {
		t.Errorf("SkillsCacheDir() = %q, want legacy %q", loc.Dir, legacy)
	}
	if !loc.Legacy {
		t.Errorf("SkillsCacheDir() Legacy = false, want true")
	}
}

func TestRelativeXDGEnvIsIgnored(t *testing.T) {
	// Per the XDG Base Directory Specification, a relative value for one
	// of these variables must be treated as unset. Isolate HOME too, so
	// this doesn't flake against a dev machine with a legacy install.
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", "relative/path")

	loc, err := SkillsConfigDir()
	if err != nil {
		t.Fatalf("SkillsConfigDir() error: %v", err)
	}
	want := filepath.Join(home, ".config", "symbrain", "skills")
	if loc.Dir != want {
		t.Errorf("SkillsConfigDir() = %q, want %q (relative XDG_CONFIG_HOME should be ignored)", loc.Dir, want)
	}
}
