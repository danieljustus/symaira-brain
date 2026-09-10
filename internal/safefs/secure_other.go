//go:build !windows

package safefs

import "os"

// OpenConfigFile opens a configuration file on platforms without the
// Windows handle-relative implementation. Unix callers use their stronger
// openat implementation in the harness package.
func OpenConfigFile(path string) (*os.File, error) {
	return os.Open(path)
}

// RemoveNoFollow is unused by the non-Windows profile implementation. It is
// provided so the platform-neutral package has one stable API.
func RemoveNoFollow(path string) error {
	return os.Remove(path)
}
