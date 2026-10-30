package usage

import (
	"bytes"
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

// The Go and Rust suites read separate copies of the shared Claude fixture.
func TestCurrentFixtureCopiesMatch(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("test source unavailable")
	}
	const name = "claude-current-cases.json"
	goFixture, err := os.ReadFile(filepath.Join(filepath.Dir(file), "testdata", name))
	if err != nil {
		t.Fatal(err)
	}
	rustFixture, err := os.ReadFile(filepath.Join(filepath.Dir(file), "../../rust/symbrain-usage/tests/fixtures", name))
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(goFixture, rustFixture) {
		t.Fatalf("fixture copy drift: %s", name)
	}
}
