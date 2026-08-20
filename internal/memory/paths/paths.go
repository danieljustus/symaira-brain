// Package paths provides shared path resolution for Symaira Memory,
// centralizing XDG Base Directory Specification compliance and
// eliminating duplicated path logic across packages.
package paths

import (
	"os"
	"path/filepath"
)

const (
	appName      = "symmemory"
	configDir    = ".config"
	dataDir      = ".local/share"
	jwtSecret    = "jwt.secret"
	databaseFile = "default.db"
)

// ConfigDir returns the application configuration directory.
// Respects XDG_CONFIG_HOME; defaults to ~/.config/symmemory.
func ConfigDir() (string, error) {
	if xdg := os.Getenv("XDG_CONFIG_HOME"); xdg != "" {
		return filepath.Join(xdg, appName), nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, configDir, appName), nil
}

// DataDir returns the application data directory.
// Respects XDG_DATA_HOME; defaults to ~/.local/share/symmemory.
func DataDir() (string, error) {
	if xdg := os.Getenv("XDG_DATA_HOME"); xdg != "" {
		return filepath.Join(xdg, appName), nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, dataDir, appName), nil
}

// SecretPath returns the full path to a named secret file
// within the config directory (e.g. ~/.config/symmemory/jwt.secret).
func SecretPath(name string) (string, error) {
	dir, err := ConfigDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, name), nil
}

// DatabasePath returns the default SQLite database path.
// Respects XDG_DATA_HOME; defaults to ~/.local/share/symmemory/default.db.
func DatabasePath() (string, error) {
	dir, err := DataDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, databaseFile), nil
}

// EnsureConfigDir creates the config directory if it doesn't exist.
func EnsureConfigDir() (string, error) {
	dir, err := ConfigDir()
	if err != nil {
		return "", err
	}
	if err := os.MkdirAll(dir, 0700); err != nil {
		return "", err
	}
	return dir, nil
}

// EnsureDataDir creates the data directory if it doesn't exist.
func EnsureDataDir() (string, error) {
	dir, err := DataDir()
	if err != nil {
		return "", err
	}
	if err := os.MkdirAll(dir, 0700); err != nil {
		return "", err
	}
	return dir, nil
}
