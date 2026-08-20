package mcptools

import (
	"os"
	"strings"
	"testing"
)

func TestServe(t *testing.T) {
	oldStdin := os.Stdin
	oldStdout := os.Stdout
	defer func() {
		os.Stdin = oldStdin
		os.Stdout = oldStdout
	}()

	r, w, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	// Closed stdin
	w.Close()
	os.Stdin = r

	// Discard stdout
	devNull, err := os.OpenFile(os.DevNull, os.O_WRONLY, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer devNull.Close()
	os.Stdout = devNull

	// Call Serve (which should return immediately on EOF of stdin)
	err = Serve("test-version", Options{})
	if err != nil && !strings.Contains(err.Error(), "short write") && !strings.Contains(err.Error(), "EOF") && !strings.Contains(err.Error(), "broken pipe") {
		t.Logf("Serve returned error: %v", err)
	}
}
