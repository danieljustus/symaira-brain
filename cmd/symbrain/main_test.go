package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func TestRun_NoArgsPrintsUsageAndExitsNoInput(t *testing.T) {
	var stdout, stderr bytes.Buffer

	code := run(nil, &stdout, &stderr)

	if code != exitcodes.ExitNoInput {
		t.Fatalf("exit code = %d, want %d", code, exitcodes.ExitNoInput)
	}
	if !strings.Contains(stdout.String(), "Usage:") {
		t.Fatalf("stdout missing usage text: %q", stdout.String())
	}
}

func TestRun_UnknownCommandExitsNoInput(t *testing.T) {
	var stdout, stderr bytes.Buffer

	code := run([]string{"bogus"}, &stdout, &stderr)

	if code != exitcodes.ExitNoInput {
		t.Fatalf("exit code = %d, want %d", code, exitcodes.ExitNoInput)
	}
	if !strings.Contains(stderr.String(), `unknown command "bogus"`) {
		t.Fatalf("stderr missing unknown-command text: %q", stderr.String())
	}
}

func TestRun_HelpExitsOK(t *testing.T) {
	for _, arg := range []string{"help", "--help", "-h"} {
		var stdout, stderr bytes.Buffer

		code := run([]string{arg}, &stdout, &stderr)

		if code != exitcodes.ExitOK {
			t.Fatalf("%s: exit code = %d, want %d", arg, code, exitcodes.ExitOK)
		}
		if !strings.Contains(stdout.String(), "Usage:") {
			t.Fatalf("%s: stdout missing usage text: %q", arg, stdout.String())
		}
	}
}

func TestRun_GlobalOutputFlagWorksBeforeOrAfterVersionCommand(t *testing.T) {
	for _, args := range [][]string{
		{"version", "--json"},
		{"version", "-json"},
		{"--json", "version"},
		{"-json", "version"},
		{"version", "--output", "json"},
		{"version", "-output", "json"},
		{"version", "--output=json"},
		{"version", "-output=json"},
	} {
		t.Run(strings.Join(args, "_"), func(t *testing.T) {
			var stdout, stderr bytes.Buffer
			code := run(args, &stdout, &stderr)
			if code != exitcodes.ExitOK {
				t.Fatalf("run(%v) = %d, want %d (stderr: %s)", args, code, exitcodes.ExitOK, stderr.String())
			}
			if !strings.HasPrefix(strings.TrimSpace(stdout.String()), "{") {
				t.Fatalf("run(%v) output = %q, want JSON", args, stdout.String())
			}
		})
	}
}

func TestRun_DoctorJSONVariantsProduceValidJSON(t *testing.T) {
	for _, args := range [][]string{
		{"doctor", "--json"},
		{"doctor", "-json"},
		{"--json", "doctor"},
		{"-json", "doctor"},
		{"doctor", "--output", "json"},
		{"doctor", "-output", "json"},
		{"doctor", "--output=json"},
		{"doctor", "-output=json"},
	} {
		t.Run(strings.Join(args, "_"), func(t *testing.T) {
			var stdout, stderr bytes.Buffer
			code := run(args, &stdout, &stderr)
			if code != exitcodes.ExitOK {
				t.Fatalf("run(%v) = %d, want %d (stderr: %s)", args, code, exitcodes.ExitOK, stderr.String())
			}
			trimmed := strings.TrimSpace(stdout.String())
			if !strings.HasPrefix(trimmed, "{") {
				t.Fatalf("run(%v) output does not start with '{': %q", args, trimmed)
			}
		})
	}
}

func TestRun_SharedFlagNormalizationAcrossSubcommands(t *testing.T) {
	// Proves that single and double dash flags work across multiple subcommands
	tests := []struct {
		name string
		args []string
	}{
		{name: "harness list double-dash", args: []string{"harness", "list", "--json"}},
		{name: "harness list single-dash", args: []string{"harness", "list", "-json"}},
		{name: "profile list double-dash", args: []string{"profile", "list", "--json"}},
		{name: "profile list single-dash", args: []string{"profile", "list", "-json"}},
		{name: "audit tail double-dash", args: []string{"audit", "tail", "--json"}},
		{name: "audit tail single-dash", args: []string{"audit", "tail", "-json"}},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// audit tail reads the audit data dir; isolate XDG state and seed
			// it so the test proves flag normalization, not a missing dir.
			t.Setenv("XDG_DATA_HOME", t.TempDir())
			t.Setenv("XDG_CONFIG_HOME", t.TempDir())
			if strings.HasPrefix(tt.name, "audit") {
				_ = os.MkdirAll(filepath.Join(os.Getenv("XDG_DATA_HOME"), "symbrain", "audit"), 0o755)
			}
			var stdout, stderr bytes.Buffer
			code := run(tt.args, &stdout, &stderr)
			if code != exitcodes.ExitOK {
				t.Fatalf("run(%v) = %d, want %d (stderr: %s)", tt.args, code, exitcodes.ExitOK, stderr.String())
			}
			trimmed := strings.TrimSpace(stdout.String())
			if !strings.HasPrefix(trimmed, "[") && !strings.HasPrefix(trimmed, "{") && trimmed != "null" {
				t.Fatalf("run(%v) output is not JSON: %q", tt.args, trimmed)
			}
		})
	}
}

func TestRun_NonOutputCommandWithOutputFlagPassesThroughToCommand(t *testing.T) {
	// init does not support global output flags, so --output json and
	// dangling --output must reach init's own flag parser (which errors
	// on the unknown flag) rather than the global output handler.
	for _, args := range [][]string{
		{"init", "--output", "json"},
		{"init", "--output"},
	} {
		t.Run(strings.Join(args, "_"), func(t *testing.T) {
			var stdout, stderr bytes.Buffer
			code := run(args, &stdout, &stderr)
			// init's own flag parser should fail with its own error, not
			// a global "symbrain: --output requires a value" message.
			if code != exitcodes.ExitNoInput {
				t.Fatalf("run(%v) = %d, want %d", args, code, exitcodes.ExitNoInput)
			}
			if !strings.Contains(stderr.String(), "init") {
				t.Fatalf("run(%v) stderr = %q, want it to reference init", args, stderr.String())
			}
			if strings.HasPrefix(stderr.String(), "symbrain: ") {
				t.Fatalf("run(%v) stderr = %q, want init's own error not global prefix", args, stderr.String())
			}
		})
	}
}

func TestRun_StubSubcommandsExitOK(t *testing.T) {
	// "init" is excluded here: it writes real files under $HOME and has its
	// own sandboxed tests in cmd_init_test.go. "profile" is excluded too: it
	// is no longer a stub (see cmd_profile.go / cmd_profile_test.go) and now
	// correctly exits ExitNoInput when called with no subcommand.
	// "install"/"uninstall" are excluded too: they are no longer stubs (see
	// cmd_install_test.go / cmd_uninstall_test.go) and correctly require
	// --harness.
	// "mcp" is excluded: it is no longer a stub (see cmd_serve.go) and
	// correctly requires --profile (the deprecated "serve" alias routes
	// to the same gateway).
	// "audit" is excluded: it is no longer a stub (see cmd_audit.go) and
	// correctly requires a subcommand.
	subcommands := []string{
		"doctor", "sync", "version",
	}

	for _, cmd := range subcommands {
		var stdout, stderr bytes.Buffer

		code := run([]string{cmd}, &stdout, &stderr)

		if code != exitcodes.ExitOK {
			t.Fatalf("%s: exit code = %d, want %d (stderr: %q)", cmd, code, exitcodes.ExitOK, stderr.String())
		}
	}
}

func TestRun_InstallUninstallDispatch_RequireHarnessFlag(t *testing.T) {
	// Bare "install"/"uninstall" (no --harness) must reach the real
	// implementation via run()'s dispatch and fail with a usage error, not
	// silently succeed like the old "not yet implemented" stub did.
	for _, cmd := range []string{"install", "uninstall"} {
		var stdout, stderr bytes.Buffer

		code := run([]string{cmd}, &stdout, &stderr)

		if code != exitcodes.ExitNoInput {
			t.Fatalf("%s: exit code = %d, want %d (stderr: %q)", cmd, code, exitcodes.ExitNoInput, stderr.String())
		}
		if !strings.Contains(stderr.String(), "--harness is required") {
			t.Fatalf("%s: stderr = %q, want it to mention --harness is required", cmd, stderr.String())
		}
	}
}

func TestRun_GuardDispatch(t *testing.T) {
	// "guard version" must reach cmdGuard via run()'s dispatch and return
	// the version output, proving the case "guard" branch at main.go:83.
	var stdout, stderr bytes.Buffer
	code := run([]string{"guard", "version"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("run(guard version) = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	if !strings.Contains(stdout.String(), "symguard") {
		t.Errorf("stdout = %q, want symguard version output", stdout.String())
	}
}
