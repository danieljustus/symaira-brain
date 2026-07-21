// Package xdg resolves the directories symbrain reads and writes: config,
// data, and cache.
package xdg

import (
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

// CacheDir returns the cache directory, respecting $XDG_CACHE_HOME;
// defaults to ~/.cache/symbrain.
func CacheDir() (string, error) {
	return resolve("XDG_CACHE_HOME", ".cache")
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
