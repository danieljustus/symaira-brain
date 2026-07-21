package main

import (
	"bytes"
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

func TestRun_StubSubcommandsExitOK(t *testing.T) {
	// "init" is excluded here: it writes real files under $HOME and has its
	// own sandboxed tests in cmd_init_test.go. "profile" is excluded too: it
	// is no longer a stub (see cmd_profile.go / cmd_profile_test.go) and now
	// correctly exits ExitNoInput when called with no subcommand.
	// "install"/"uninstall" are excluded too: they are no longer stubs (see
	// cmd_install_test.go / cmd_uninstall_test.go) and correctly require
	// --harness.
	// "serve" is excluded: it is no longer a stub (see cmd_serve.go) and
	// correctly requires --profile.
	subcommands := []string{
		"doctor", "sync", "audit", "version",
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
