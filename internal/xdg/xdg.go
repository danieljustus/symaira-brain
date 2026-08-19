// Package xdg resolves the directories symbrain reads and writes: config,
// data, and cache.
package xdg

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-corekit/configkit"
)

// ConfigDir returns ~/.config/symbrain. This intentionally reuses
// corekit/configkit's own path resolution (which does not consult
// $XDG_CONFIG_HOME) so that a config file written here is always the exact
// file config.Load() reads back.
func ConfigDir() string {
	return filepath.Dir(ConfigPath())
}

// ConfigPath returns ~/.config/symbrain/config.toml.
func ConfigPath() string {
	return configkit.DefaultPath(config.AppName)
}

// ProfilesDir returns ~/.config/symbrain/profiles.
func ProfilesDir() string {
	return filepath.Join(ConfigDir(), "profiles")
}

// DataDir returns the data directory, respecting $XDG_DATA_HOME; defaults
// to ~/.local/share/symbrain.
func DataDir() (string, error) {
	return resolve("XDG_DATA_HOME", filepath.Join(".local", "share"))
}

// AuditDir returns the audit log directory under DataDir
// (~/.local/share/symbrain/audit).
func AuditDir() (string, error) {
	dir, err := DataDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, "audit"), nil
}

// RecipesDir returns the episode store directory under DataDir
// (~/.local/share/symbrain/recipes).
func RecipesDir() (string, error) {
	dir, err := DataDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, "recipes"), nil
}

// CacheDir returns the cache directory, respecting $XDG_CACHE_HOME;
// defaults to ~/.cache/symbrain.
func CacheDir() (string, error) {
	return resolve("XDG_CACHE_HOME", ".cache")
}

// ManagedBinDir returns ~/.symaira/bin, the directory where managed core
// binaries (symvault, symmemory, symskills) are installed by
// `symbrain setup` and repaired by `symbrain doctor --fix`.
//
// This directory is checked before PATH/exec.LookPath during binary
// resolution, giving managed binaries priority over Homebrew or other
// system installations.
func ManagedBinDir() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", fmt.Errorf("managed: cannot determine home directory: %w", err)
	}
	return filepath.Join(home, ".symaira", "bin"), nil
}

func resolve(envVar, fallbackRel string) (string, error) {
	if v := os.Getenv(envVar); v != "" {
		return filepath.Join(v, config.AppName), nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, fallbackRel, config.AppName), nil
}
