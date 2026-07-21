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
	// own sandboxed tests in cmd_init_test.go. "profile" is excluded too:
	// it is no longer a stub (see cmd_profile.go / cmd_profile_test.go) and
	// now correctly exits ExitNoInput when called with no subcommand.
	subcommands := []string{
		"doctor", "serve",
		"install", "uninstall", "sync", "audit", "version",
	}

	for _, cmd := range subcommands {
		var stdout, stderr bytes.Buffer

		code := run([]string{cmd}, &stdout, &stderr)

		if code != exitcodes.ExitOK {
			t.Fatalf("%s: exit code = %d, want %d (stderr: %q)", cmd, code, exitcodes.ExitOK, stderr.String())
		}
	}
}
