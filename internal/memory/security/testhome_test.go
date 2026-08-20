package security

import (
	"os"
	"testing"
)

// setTestHome redirects HOME (Unix) and USERPROFILE (Windows) to dir so
// tests never touch the real user config/data directories. On Windows,
// os.UserHomeDir reads USERPROFILE, not HOME — setting only HOME leaks
// state into the real user profile and breaks test isolation.
func setTestHome(t *testing.T, dir string) {
	t.Helper()
	oldHome := os.Getenv("HOME")
	oldProfile := os.Getenv("USERPROFILE")
	os.Setenv("HOME", dir)
	os.Setenv("USERPROFILE", dir)
	t.Cleanup(func() {
		os.Setenv("HOME", oldHome)
		os.Setenv("USERPROFILE", oldProfile)
	})
}
