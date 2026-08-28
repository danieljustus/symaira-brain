package main

import (
	"bytes"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// documentedCommands is the source-of-truth list of top-level commands
// advertised by the CLI. It must stay in sync with the command reference
// in README.md and AGENTS.md.
var documentedCommands = []string{
	"init",
	"doctor",
	"setup",
	"profile",
	"config",
	"harness",
	"usage",
	"mcp",
	"install",
	"uninstall",
	"sync",
	"memory",
	"audit",
	"vault",
	"guard",
	"version",
	"help",
}

func TestHelpMatchesDocumentedCommands(t *testing.T) {
	var stdout, stderr bytes.Buffer
	code := run([]string{"help"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("help exit code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	helpText := stdout.String()
	start := strings.Index(helpText, "Commands:\n")
	if start == -1 {
		t.Fatalf("help output missing Commands section:\n%s", helpText)
	}

	lines := strings.Split(helpText[start+len("Commands:\n"):], "\n")
	var actual []string
	for _, line := range lines {
		line = strings.TrimRight(line, "\r")
		trimmed := strings.TrimLeft(line, " ")
		if trimmed == "" {
			continue
		}
		if !strings.HasPrefix(line, "  ") {
			break
		}
		fields := strings.Fields(trimmed)
		if len(fields) == 0 {
			continue
		}
		actual = append(actual, fields[0])
	}

	if len(actual) != len(documentedCommands) {
		t.Fatalf("command count mismatch: got %d (%v), want %d (%v)", len(actual), actual, len(documentedCommands), documentedCommands)
	}
	for i := range actual {
		if actual[i] != documentedCommands[i] {
			t.Fatalf("command mismatch at index %d: got %q, want %q", i, actual[i], documentedCommands[i])
		}
	}
}
