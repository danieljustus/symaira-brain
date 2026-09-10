package main

import (
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

type profileRemoveFailWriter struct{}

func (profileRemoveFailWriter) Write([]byte) (int, error) {
	return 0, io.ErrClosedPipe
}

type profileRemoveTrackingReader struct {
	reads int
}

func (r *profileRemoveTrackingReader) Read([]byte) (int, error) {
	r.reads++
	return 0, io.EOF
}

func TestCmdProfileRemove_PromptWriteFailureFailsClosed(t *testing.T) {
	home := sandboxHome(t)
	writeProfileFile(t, home, "write-failure", `[profile]
name = "write-failure"`)

	reader := &profileRemoveTrackingReader{}
	oldReader := confirmReader
	confirmReader = reader
	t.Cleanup(func() { confirmReader = oldReader })

	var stderr strings.Builder
	code := cmdProfile([]string{"remove", "write-failure"}, profileRemoveFailWriter{}, &stderr)
	if code != exitcodes.ExitGeneric {
		t.Fatalf("profile remove with failing stdout = %d, want %d (stderr: %s)", code, exitcodes.ExitGeneric, stderr.String())
	}
	if reader.reads != 0 {
		t.Fatalf("confirmation reader was read %d times after prompt write failure, want 0", reader.reads)
	}
	if !profileExistsForTest(t, home, "write-failure") {
		t.Fatal("profile was removed despite prompt write failure")
	}
}

func profileExistsForTest(t *testing.T, home, name string) bool {
	t.Helper()
	_, err := os.Stat(filepath.Join(home, ".config", "symbrain", "profiles", name+".toml"))
	return err == nil
}
