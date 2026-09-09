package contextassembler

import (
	"os"
	"testing"
)

// TestMain keeps context-assembly tests away from a developer's real memory
// database. The package uses config.Defaults() in several tests, so relying on
// each test to provide a database path is easy to get wrong and makes the
// result depend on prior local migrations.
func TestMain(m *testing.M) {
	dataDir, err := os.MkdirTemp("", "symbrain-contextassembler-")
	if err != nil {
		panic(err)
	}
	if err := os.Setenv("XDG_DATA_HOME", dataDir); err != nil {
		_ = os.RemoveAll(dataDir)
		panic(err)
	}

	status := m.Run()
	_ = os.Unsetenv("XDG_DATA_HOME")
	_ = os.RemoveAll(dataDir)
	os.Exit(status)
}
