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
	code := cmdPassthrough("vault", []string{"--help"}, strings.NewReader(""), &stdout, &stderr)
	if code == exitcodes.ExitOK {
		t.Fatal("expected non-zero exit for missing binary")
	}
	if stdout.Len() != 0 {
		t.Fatalf("stdout = %q, want empty", stdout.String())
	}
	if !strings.Contains(stderr.String(), "Hint: install symvault") {
		t.Fatalf("stderr = %q, want installation hint", stderr.String())
	}
}

// TestCmdPassthrough_FakeBinary verifies that a passthrough subcommand
// runs the correct binary with the given args and preserves stdout.
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

	var stdout, stderr strings.Builder
	code := cmdPassthrough("vault", []string{"--version"}, strings.NewReader(""), &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("cmdPassthrough(vault) = %v, stderr: %s", code, stderr.String())
	}
	if got, want := stdout.String(), "called:--version\n"; got != want {
		t.Fatalf("stdout = %q, want %q", got, want)
	}
	if stderr.Len() != 0 {
		t.Fatalf("stderr = %q, want empty", stderr.String())
	}
}

func TestRun_VaultApprovalPassthroughPreservesChildContract(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("skip shell fixture test on Windows")
	}

	binDir := t.TempDir()
	fakeBin := filepath.Join(binDir, "symvault")
	script := `#!/bin/sh
printf '%s\n' "$@" > "$SYMBRAIN_TEST_ARGV"
printf '%s' "$SYMBRAIN_TEST_STDOUT"
printf '%s' "$SYMBRAIN_TEST_STDERR" >&2
exit "${SYMBRAIN_TEST_EXIT:-0}"
`
	if err := os.WriteFile(fakeBin, []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}

	t.Setenv("HOME", t.TempDir())
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())
	t.Setenv("PATH", binDir)

	tests := []struct {
		name        string
		args        []string
		childStdout string
		childStderr string
		childExit   string
		wantCode    exitcodes.ExitCode
	}{
		{
			name:        "list JSON",
			args:        []string{"approval", "list", "--output", "json"},
			childStdout: `{"requests":[{"id":"apr-test","status":"pending"}]}` + "\n",
			wantCode:    exitcodes.ExitOK,
		},
		{
			name:        "decide JSON",
			args:        []string{"approval", "decide", "apr-test", "--deny", "--json"},
			childStdout: `{"outcome":{"id":"apr-test","status":"denied"}}` + "\n",
			wantCode:    exitcodes.ExitOK,
		},
		{
			name:        "child failure",
			args:        []string{"approval", "decide", "apr-missing", "--approve"},
			childStderr: "symvault: approval request not found\n",
			childExit:   "42",
			wantCode:    exitcodes.ExitCode(42),
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			argvPath := filepath.Join(t.TempDir(), "argv")
			t.Setenv("SYMBRAIN_TEST_ARGV", argvPath)
			t.Setenv("SYMBRAIN_TEST_STDOUT", tt.childStdout)
			t.Setenv("SYMBRAIN_TEST_STDERR", tt.childStderr)
			t.Setenv("SYMBRAIN_TEST_EXIT", tt.childExit)

			var stdout, stderr strings.Builder
			code := run(append([]string{"vault"}, tt.args...), &stdout, &stderr)
			if code != tt.wantCode {
				t.Fatalf("run(vault %v) = %v, want %v", tt.args, code, tt.wantCode)
			}
			if got, want := stdout.String(), tt.childStdout; got != want {
				t.Fatalf("stdout = %q, want child stdout %q", got, want)
			}
			if got, want := stderr.String(), tt.childStderr; got != want {
				t.Fatalf("stderr = %q, want child stderr %q", got, want)
			}

			gotArgv, err := os.ReadFile(argvPath)
			if err != nil {
				t.Fatalf("read recorded argv: %v", err)
			}
			wantArgv := strings.Join(tt.args, "\n") + "\n"
			if got, want := string(gotArgv), wantArgv; got != want {
				t.Fatalf("child argv = %q, want %q", got, want)
			}
		})
	}
}

func TestHelpDocumentsVaultApprovalPassthrough(t *testing.T) {
	var stdout, stderr strings.Builder
	if code := run([]string{"help"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("help exit code = %v, want %v (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	for _, want := range []string{
		"symbrain vault approval list [--output json]",
		"symbrain vault approval decide <request-id> --approve|--deny",
	} {
		if !strings.Contains(stdout.String(), want) {
			t.Errorf("help does not document %q:\n%s", want, stdout.String())
		}
	}
	if stderr.Len() != 0 {
		t.Fatalf("help stderr = %q, want empty", stderr.String())
	}
}

// TestCmdPassthrough_UnknownSubcmd verifies that an unknown subcommand name
// returns ExitNoInput.
func TestCmdPassthrough_UnknownSubcmd(t *testing.T) {
	var stdout, stderr strings.Builder
	code := cmdPassthrough("unknown", nil, strings.NewReader(""), &stdout, &stderr)
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
	fakeBin := filepath.Join(binDir, "symvault")
	script := "#!/bin/sh\nexit 42\n"
	if err := os.WriteFile(fakeBin, []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", binDir)

	var stdout, stderr strings.Builder
	code := cmdPassthrough("vault", nil, strings.NewReader(""), &stdout, &stderr)
	if code != 42 {
		t.Fatalf("cmdPassthrough(vault) = %v, want 42", code)
	}
	if stdout.Len() != 0 || stderr.Len() != 0 {
		t.Fatalf("unexpected child output: stdout=%q stderr=%q", stdout.String(), stderr.String())
	}
}

// TestPassthroughMapKeys verifies the expected subcommands exist.
func TestPassthroughMapKeys(t *testing.T) {
	expected := []string{"vault"}
	for _, name := range expected {
		if _, ok := passthroughMap[name]; !ok {
			t.Errorf("passthroughMap missing key %q", name)
		}
	}
	if len(passthroughMap) != len(expected) {
		t.Errorf("passthroughMap has %d keys, want %d", len(passthroughMap), len(expected))
	}
}

func TestCmdPassthrough_EmbeddedCoresAreNotPassthroughs(t *testing.T) {
	var stdout, stderr strings.Builder
	for _, name := range []string{"memory", "skills"} {
		if code := cmdPassthrough(name, nil, strings.NewReader(""), &stdout, &stderr); code != exitcodes.ExitNoInput {
			t.Errorf("cmdPassthrough(%q) = %v, want %v", name, code, exitcodes.ExitNoInput)
		}
	}
}
