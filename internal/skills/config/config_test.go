package config

import (
	"os"
	"path/filepath"
	"testing"
)

func TestDefaults(t *testing.T) {
	cfg := Defaults()

	// Verify all paths are set
	if cfg.LibraryDir == "" {
		t.Error("LibraryDir should not be empty")
	}
	if cfg.RenderDir == "" {
		t.Error("RenderDir should not be empty")
	}
	if cfg.CacheDir == "" {
		t.Error("CacheDir should not be empty")
	}
	if cfg.ProfilesDir == "" {
		t.Error("ProfilesDir should not be empty")
	}
	if cfg.BaseDir == "" {
		t.Error("BaseDir should not be empty")
	}

	// Verify paths contain expected components
	home, _ := os.UserHomeDir()
	expectedLib := filepath.Join(home, ".local", "share", "symskills", "library")
	expectedRender := filepath.Join(home, ".local", "share", "symskills", "rendered")
	expectedCache := filepath.Join(home, ".cache", "symskills")
	expectedProfiles := filepath.Join(home, ".config", "symskills", "profiles")
	expectedBase := filepath.Join(home, ".local", "share", "symskills", "base")

	if cfg.LibraryDir != expectedLib {
		t.Errorf("LibraryDir = %q, want %q", cfg.LibraryDir, expectedLib)
	}
	if cfg.RenderDir != expectedRender {
		t.Errorf("RenderDir = %q, want %q", cfg.RenderDir, expectedRender)
	}
	if cfg.CacheDir != expectedCache {
		t.Errorf("CacheDir = %q, want %q", cfg.CacheDir, expectedCache)
	}
	if cfg.ProfilesDir != expectedProfiles {
		t.Errorf("ProfilesDir = %q, want %q", cfg.ProfilesDir, expectedProfiles)
	}
	if cfg.BaseDir != expectedBase {
		t.Errorf("BaseDir = %q, want %q", cfg.BaseDir, expectedBase)
	}
}

func TestConfigPath(t *testing.T) {
	path := ConfigPath()

	// Verify path is not empty
	if path == "" {
		t.Error("ConfigPath should not be empty")
	}

	// Verify path contains expected components
	home, _ := os.UserHomeDir()
	expected := filepath.Join(home, ".config", "symskills", "config.toml")
	if path != expected {
		t.Errorf("ConfigPath() = %q, want %q", path, expected)
	}

	// Verify path ends with config.toml
	if filepath.Ext(path) != ".toml" {
		t.Errorf("ConfigPath should end with .toml, got %q", path)
	}
}

func TestEnsureDirs(t *testing.T) {
	// Create a temporary directory for testing
	tmpDir := t.TempDir()

	// Override home directory for testing
	origHome := os.Getenv("HOME")
	os.Setenv("HOME", tmpDir)
	defer os.Setenv("HOME", origHome)

	cfg := &Config{
		LibraryDir:  filepath.Join(tmpDir, "library"),
		RenderDir:   filepath.Join(tmpDir, "rendered"),
		CacheDir:    filepath.Join(tmpDir, "cache"),
		ProfilesDir: filepath.Join(tmpDir, "profiles"),
		BaseDir:     filepath.Join(tmpDir, "base"),
	}

	// Ensure directories
	err := EnsureDirs(cfg)
	if err != nil {
		t.Fatalf("EnsureDirs failed: %v", err)
	}

	// Verify directories were created
	dirs := []string{
		filepath.Dir(ConfigPath()),
		cfg.LibraryDir,
		cfg.RenderDir,
		cfg.CacheDir,
		cfg.ProfilesDir,
		cfg.BaseDir,
	}

	for _, dir := range dirs {
		if _, err := os.Stat(dir); os.IsNotExist(err) {
			t.Errorf("Directory %q was not created", dir)
		}
	}
}

func TestEnsureDirsPermissions(t *testing.T) {
	// Create a temporary directory for testing
	tmpDir := t.TempDir()

	// Override home directory for testing
	origHome := os.Getenv("HOME")
	os.Setenv("HOME", tmpDir)
	defer os.Setenv("HOME", origHome)

	cfg := &Config{
		LibraryDir:  filepath.Join(tmpDir, "library"),
		RenderDir:   filepath.Join(tmpDir, "rendered"),
		CacheDir:    filepath.Join(tmpDir, "cache"),
		ProfilesDir: filepath.Join(tmpDir, "profiles"),
		BaseDir:     filepath.Join(tmpDir, "base"),
	}

	// Ensure directories
	err := EnsureDirs(cfg)
	if err != nil {
		t.Fatalf("EnsureDirs failed: %v", err)
	}

	// Verify directory permissions
	dirs := []string{
		filepath.Dir(ConfigPath()),
		cfg.LibraryDir,
		cfg.RenderDir,
		cfg.CacheDir,
		cfg.ProfilesDir,
		cfg.BaseDir,
	}

	for _, dir := range dirs {
		info, err := os.Stat(dir)
		if err != nil {
			t.Errorf("Failed to stat %q: %v", dir, err)
			continue
		}
		if info.Mode().Perm() != 0o755 {
			t.Errorf("Directory %q has permissions %o, want 0755", dir, info.Mode().Perm())
		}
	}
}

func TestLoad(t *testing.T) {
	// Create a temporary directory for testing
	tmpDir := t.TempDir()

	// Override home directory for testing
	origHome := os.Getenv("HOME")
	os.Setenv("HOME", tmpDir)
	defer os.Setenv("HOME", origHome)

	// Ensure directories exist
	cfg := Defaults()
	err := EnsureDirs(cfg)
	if err != nil {
		t.Fatalf("EnsureDirs failed: %v", err)
	}

	// Load config (should load defaults)
	loaded, err := Load()
	if err != nil {
		t.Fatalf("Load failed: %v", err)
	}

	// Verify loaded config matches defaults
	defaults := Defaults()
	if loaded.LibraryDir != defaults.LibraryDir {
		t.Errorf("LibraryDir = %q, want %q", loaded.LibraryDir, defaults.LibraryDir)
	}
	if loaded.RenderDir != defaults.RenderDir {
		t.Errorf("RenderDir = %q, want %q", loaded.RenderDir, defaults.RenderDir)
	}
	if loaded.CacheDir != defaults.CacheDir {
		t.Errorf("CacheDir = %q, want %q", loaded.CacheDir, defaults.CacheDir)
	}
	if loaded.ProfilesDir != defaults.ProfilesDir {
		t.Errorf("ProfilesDir = %q, want %q", loaded.ProfilesDir, defaults.ProfilesDir)
	}
}

func TestLoadWithCustomConfig(t *testing.T) {
	// Create a temporary directory for testing
	tmpDir := t.TempDir()

	// Override home directory for testing
	origHome := os.Getenv("HOME")
	os.Setenv("HOME", tmpDir)
	defer os.Setenv("HOME", origHome)

	// Ensure directories exist
	cfg := Defaults()
	err := EnsureDirs(cfg)
	if err != nil {
		t.Fatalf("EnsureDirs failed: %v", err)
	}

	// Create a custom config file
	configPath := ConfigPath()
	customContent := `library_dir = "/custom/library"
render_dir = "/custom/rendered"
cache_dir = "/custom/cache"
profiles_dir = "/custom/profiles"
base_dir = "/custom/base"
`
	err = os.WriteFile(configPath, []byte(customContent), 0644)
	if err != nil {
		t.Fatalf("Failed to write config file: %v", err)
	}

	// Load config
	loaded, err := Load()
	if err != nil {
		t.Fatalf("Load failed: %v", err)
	}

	// Verify loaded config has custom values
	if loaded.LibraryDir != "/custom/library" {
		t.Errorf("LibraryDir = %q, want %q", loaded.LibraryDir, "/custom/library")
	}
	if loaded.RenderDir != "/custom/rendered" {
		t.Errorf("RenderDir = %q, want %q", loaded.RenderDir, "/custom/rendered")
	}
	if loaded.CacheDir != "/custom/cache" {
		t.Errorf("CacheDir = %q, want %q", loaded.CacheDir, "/custom/cache")
	}
	if loaded.ProfilesDir != "/custom/profiles" {
		t.Errorf("ProfilesDir = %q, want %q", loaded.ProfilesDir, "/custom/profiles")
	}
	if loaded.BaseDir != "/custom/base" {
		t.Errorf("BaseDir = %q, want %q", loaded.BaseDir, "/custom/base")
	}
}

func TestLoadWithPartialConfig(t *testing.T) {
	// Create a temporary directory for testing
	tmpDir := t.TempDir()

	// Override home directory for testing
	origHome := os.Getenv("HOME")
	os.Setenv("HOME", tmpDir)
	defer os.Setenv("HOME", origHome)

	// Ensure directories exist
	cfg := Defaults()
	err := EnsureDirs(cfg)
	if err != nil {
		t.Fatalf("EnsureDirs failed: %v", err)
	}

	// Create a partial config file (only library_dir)
	configPath := ConfigPath()
	customContent := `library_dir = "/custom/library"
`
	err = os.WriteFile(configPath, []byte(customContent), 0644)
	if err != nil {
		t.Fatalf("Failed to write config file: %v", err)
	}

	// Load config
	loaded, err := Load()
	if err != nil {
		t.Fatalf("Load failed: %v", err)
	}

	// Verify loaded config has custom library_dir but defaults for others
	if loaded.LibraryDir != "/custom/library" {
		t.Errorf("LibraryDir = %q, want %q", loaded.LibraryDir, "/custom/library")
	}

	defaults := Defaults()
	if loaded.RenderDir != defaults.RenderDir {
		t.Errorf("RenderDir = %q, want %q (should use default)", loaded.RenderDir, defaults.RenderDir)
	}
	if loaded.CacheDir != defaults.CacheDir {
		t.Errorf("CacheDir = %q, want %q (should use default)", loaded.CacheDir, defaults.CacheDir)
	}
	if loaded.ProfilesDir != defaults.ProfilesDir {
		t.Errorf("ProfilesDir = %q, want %q (should use default)", loaded.ProfilesDir, defaults.ProfilesDir)
	}
}

func TestEnsureDirsWithEmptyDir(t *testing.T) {
	tmpDir := t.TempDir()

	origHome := os.Getenv("HOME")
	os.Setenv("HOME", tmpDir)
	defer os.Setenv("HOME", origHome)

	cfg := &Config{
		LibraryDir:  filepath.Join(tmpDir, "library"),
		RenderDir:   filepath.Join(tmpDir, "rendered"),
		CacheDir:    filepath.Join(tmpDir, "cache"),
		ProfilesDir: "",
	}

	err := EnsureDirs(cfg)
	if err != nil {
		t.Fatalf("EnsureDirs with empty ProfilesDir failed: %v", err)
	}

	dirs := []string{
		filepath.Dir(ConfigPath()),
		cfg.LibraryDir,
		cfg.RenderDir,
		cfg.CacheDir,
	}

	for _, dir := range dirs {
		if _, err := os.Stat(dir); os.IsNotExist(err) {
			t.Errorf("Directory %q was not created", dir)
		}
	}
}
