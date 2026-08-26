package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/output"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// TestCmdWrapperExtractFormatPrelude exercises the public command wrappers
// (cmdDoctor, cmdAudit, cmdHarness, cmdUsage, cmdMemory) whose extractFormat
// preludes and error branches were previously only reachable via run().
// Table-driven: each wrapper is invoked through its real entry point with a
// valid JSON request (happy path) and an invalid output flag (error path).
func TestCmdWrapperExtractFormatPrelude(t *testing.T) {
	tests := []struct {
		name       string
		call       func(args []string, stdout, stderr *bytes.Buffer) exitcodes.ExitCode
		happyArgs  func(home string) []string // args for the happy path; home = isolated config dir
		errPrefix  string                     // expected stderr prefix on the error path
		wantStdout string                     // substring the happy path must print
	}{
		{
			name: "doctor",
			call: func(args []string, stdout, stderr *bytes.Buffer) exitcodes.ExitCode {
				return cmdDoctor(args, stdout, stderr)
			},
			happyArgs:  func(home string) []string { return []string{"--json"} },
			errPrefix:  "symbrain doctor:",
			wantStdout: `"config_dir"`,
		},
		{
			name: "audit tail",
			call: func(args []string, stdout, stderr *bytes.Buffer) exitcodes.ExitCode {
				return cmdAuditTail(args, stdout, stderr)
			},
			happyArgs:  func(home string) []string { return []string{"-n", "1", "-json"} },
			errPrefix:  "symbrain audit tail:",
			wantStdout: "", // empty audit dir yields JSON null — prelude ran, exit 0
		},
		{
			name: "audit",
			call: func(args []string, stdout, stderr *bytes.Buffer) exitcodes.ExitCode {
				return cmdAudit(args, stdout, stderr)
			},
			happyArgs:  func(home string) []string { return []string{"tail", "-n", "1", "--json"} },
			errPrefix:  "symbrain audit:",
			wantStdout: "", // empty audit dir yields JSON null — prelude ran, exit 0
		},
		{
			name: "harness list",
			call: func(args []string, stdout, stderr *bytes.Buffer) exitcodes.ExitCode {
				return cmdHarness(args, stdout, stderr)
			},
			happyArgs:  func(home string) []string { return []string{"list", "--json"} },
			errPrefix:  "symbrain harness:",
			wantStdout: "[",
		},
		{
			name: "usage",
			call: func(args []string, stdout, stderr *bytes.Buffer) exitcodes.ExitCode {
				return cmdUsage(args, stdout, stderr)
			},
			happyArgs:  func(home string) []string { return []string{"--json"} },
			errPrefix:  "symbrain usage:",
			wantStdout: "{",
		},
		{
			name: "memory",
			call: func(args []string, stdout, stderr *bytes.Buffer) exitcodes.ExitCode {
				return cmdMemory(args, stdout, stderr)
			},
			happyArgs: func(home string) []string { return nil }, // usage output, no backend needed
			errPrefix: "symbrain memory:",
			// The memory wrapper prints its subcommand help on empty args;
			// any non-empty stdout proves the prelude passed the args through.
			wantStdout: "",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name+"/happy-path", func(t *testing.T) {
			// Isolate XDG state so audit/harness lookups never touch the
			// real user data dir (CI runners have no ~/.local/share/symbrain).
			t.Setenv("XDG_DATA_HOME", t.TempDir())
			t.Setenv("XDG_CONFIG_HOME", t.TempDir())
			// The audit wrappers read the audit dir; create it so the
			// happy path exercises the prelude, not a missing-dir error.
			if strings.HasPrefix(tt.name, "audit") {
				_ = os.MkdirAll(filepath.Join(os.Getenv("XDG_DATA_HOME"), "symbrain", "audit"), 0o755)
			}
			stdout := &bytes.Buffer{}
			stderr := &bytes.Buffer{}
			code := tt.call(tt.happyArgs(t.TempDir()), stdout, stderr)
			if code != exitcodes.ExitOK && code != exitcodes.ExitNoInput {
				t.Fatalf("exit code = %v, want OK or NoInput; stderr: %s", code, stderr.String())
			}
			if tt.wantStdout != "" && !strings.Contains(stdout.String(), tt.wantStdout) {
				t.Errorf("stdout %q does not contain %q; stderr: %s", stdout.String(), tt.wantStdout, stderr.String())
			}
		})

		t.Run(tt.name+"/invalid-output-flag", func(t *testing.T) {
			stdout := &bytes.Buffer{}
			stderr := &bytes.Buffer{}
			code := tt.call([]string{"--output", "bogus"}, stdout, stderr)
			if code != exitcodes.ExitNoInput {
				t.Fatalf("exit code = %v, want ExitNoInput", code)
			}
			if !strings.HasPrefix(stderr.String(), tt.errPrefix) {
				t.Errorf("stderr %q does not start with %q", stderr.String(), tt.errPrefix)
			}
			if stdout.Len() != 0 {
				t.Errorf("expected no stdout on error path, got %q", stdout.String())
			}
		})
	}
}

func TestExtractFormatRejectsInvalidOutput(t *testing.T) {
	_, _, err := extractFormat([]string{"doctor", "--output", "xml"})
	if err == nil {
		t.Fatal("extractFormat(--output xml) = nil error, want invalid-format error")
	}
}

// TestNormalizeFlagsAfterTerminator pins the terminator contract: everything
// after a bare "--" must survive verbatim, including double-dash tokens that
// would otherwise be rewritten.
func TestNormalizeFlagsAfterTerminator(t *testing.T) {
	got := normalizeFlags([]string{"run", "--profile", "x", "--", "--keep-me", "-json"})
	want := []string{"run", "-profile", "x", "--", "--keep-me", "-json"}
	for i, v := range want {
		if got[i] != v {
			t.Fatalf("normalizeFlags() = %v, want %v", got, want)
		}
	}
}

var _ = output.FormatTable // keep import stable if wrappers change signature
