// Package paths provides Symaira Memory's path resolution, delegating to
// the shared internal/paths resolver so memory and skills agree on one XDG
// Base Directory convention under the symbrain app name (#427).
package paths

import (
	"os"
	"path/filepath"

	sharedpaths "github.com/danieljustus/symaira-brain/internal/paths"
)

const databaseFile = "default.db"

// ConfigDir returns the application configuration directory: the current
// ~/.config/symbrain/memory (respecting $XDG_CONFIG_HOME), or the legacy
// ~/.config/symmemory when only that exists on disk. See ConfigLocation
// for a variant that also reports which of the two was used.
func ConfigDir() (string, error) {
	loc, err := ConfigLocation()
	if err != nil {
		return "", err
	}
	return loc.Dir, nil
}

// DataDir returns the application data directory: the current
// ~/.local/share/symbrain/memory (respecting $XDG_DATA_HOME), or the
// legacy ~/.local/share/symmemory when only that exists on disk. See
// DataLocation for a variant that also reports which of the two was used.
func DataDir() (string, error) {
	loc, err := DataLocation()
	if err != nil {
		return "", err
	}
	return loc.Dir, nil
}

// ConfigLocation resolves the configuration directory and reports whether
// it is the current symbrain-namespaced location or a legacy symmemory
// install. Exported as a clean, testable entry point for a future
// `symbrain doctor` (#425) to surface a migration hint from; that
// reporting is out of scope here.
func ConfigLocation() (sharedpaths.Location, error) {
	return sharedpaths.MemoryConfigDir()
}

// DataLocation resolves the data directory and reports whether it is the
// current symbrain-namespaced location or a legacy symmemory install.
func DataLocation() (sharedpaths.Location, error) {
	return sharedpaths.MemoryDataDir()
}

// SecretPath returns the full path to a named secret file
// within the config directory (e.g. .../symbrain/memory/jwt.secret, or the
// legacy .../symmemory/jwt.secret when only that exists).
func SecretPath(name string) (string, error) {
	dir, err := ConfigDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, name), nil
}

// DatabasePath returns the default SQLite database path, under DataDir.
func DatabasePath() (string, error) {
	dir, err := DataDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, databaseFile), nil
}

// EnsureConfigDir creates the config directory if it doesn't exist and
// returns it. When resolution fell back to a legacy symmemory directory
// that already exists, this is a no-op beyond the resolution itself.
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

// EnsureDataDir creates the data directory if it doesn't exist and returns
// it. When resolution fell back to a legacy symmemory directory that
// already exists, this is a no-op beyond the resolution itself.
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
