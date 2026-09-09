package main

import (
	"bytes"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func TestCmdSync_TargetErrorStillRendersCompleteResult(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("symlink setup is not available on every Windows runner")
	}
	project := t.TempDir()
	t.Setenv("HOME", t.TempDir())
	outside := filepath.Join(t.TempDir(), "outside.md")
	if err := os.WriteFile(outside, []byte("outside\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(project, "CLAUDE.md")); err != nil {
		t.Fatal(err)
	}

	var stdout, stderr bytes.Buffer
	code := cmdSyncWithFormat(
		[]string{"--project", project, "claude", "agents"},
		&stdout,
		&stderr,
		output.FormatJSON,
	)
	if code != exitcodes.ExitGeneric {
		t.Fatalf("cmdSyncWithFormat() = %d, want generic; stdout=%q stderr=%q", code, stdout.String(), stderr.String())
	}
	for _, want := range []string{`"name":"claude"`, `"status":"error"`, `"name":"agents"`, `"status":"created"`} {
		if !strings.Contains(stdout.String(), want) {
			t.Fatalf("JSON result missing %q: %s", want, stdout.String())
		}
	}
	if !strings.Contains(stderr.String(), "one or more instruction targets failed") {
		t.Fatalf("stderr = %q, want aggregate target error", stderr.String())
	}
}
