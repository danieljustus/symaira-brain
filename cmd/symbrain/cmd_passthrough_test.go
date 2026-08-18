package main

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// TestCmdPassthrough_VaultNotFound verifies that running "symbrain vault"
// without symvault installed produces a clear error message.
func TestCmdPassthrough_VaultNotFound(t *testing.T) {
	// Ensure symvault is NOT on PATH for this test.
	t.Setenv("PATH", t.TempDir())

	var stdout, stderr strings.Builder
	code := cmdPassthrough("vault", []string{"--help"}, &stderr)
	if code == exitcodes.ExitOK {
		t.Fatal("expected non-zero exit for missing binary")
	}
	_ = stdout // unused — passthrough writes directly to os.Stdout
	_ = stderr
}

// TestCmdPassthrough_FakeBinary verifies that a passthrough subcommand
// exec's the correct binary with the given args and propagates the exit code.
func TestCmdPassthrough_FakeBinary(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("skip exec test on Windows")
	}

	// Create a fake "symvault" script that prints its args and exits 0.
	binDir := t.TempDir()
	fakeBin := filepath.Join(binDir, "symvault")
	script := "#!/bin/sh\necho \"called:$*\"\n"
	if err := os.WriteFile(fakeBin, []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", binDir)

	// We can't capture stdout from cmdPassthrough (it writes to os.Stdout),
	// but we can verify it exits OK and doesn't error.
	var stderr strings.Builder
	code := cmdPassthrough("vault", []string{"--version"}, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("cmdPassthrough(vault) = %v, stderr: %s", code, stderr.String())
	}
}

// TestCmdPassthrough_UnknownSubcmd verifies that an unknown subcommand name
// returns ExitNoInput.
func TestCmdPassthrough_UnknownSubcmd(t *testing.T) {
	var stderr strings.Builder
	code := cmdPassthrough("unknown", nil, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("cmdPassthrough(unknown) = %v, want %v", code, exitcodes.ExitNoInput)
	}
}

// TestCmdPassthrough_ExitCodePropagation verifies that the child's non-zero
// exit code is propagated.
func TestCmdPassthrough_ExitCodePropagation(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("skip exec test on Windows")
	}

	binDir := t.TempDir()
	fakeBin := filepath.Join(binDir, "symmemory")
	script := "#!/bin/sh\nexit 42\n"
	if err := os.WriteFile(fakeBin, []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", binDir)

	var stderr strings.Builder
	code := cmdPassthrough("memory", nil, &stderr)
	if code != 42 {
		t.Fatalf("cmdPassthrough(memory) = %v, want 42", code)
	}
}

// TestPassthroughMapKeys verifies the expected subcommands exist.
func TestPassthroughMapKeys(t *testing.T) {
	expected := []string{"vault", "memory", "skills"}
	for _, name := range expected {
		if _, ok := passthroughMap[name]; !ok {
			t.Errorf("passthroughMap missing key %q", name)
		}
	}
	if len(passthroughMap) != len(expected) {
		t.Errorf("passthroughMap has %d keys, want %d", len(passthroughMap), len(expected))
	}
}
