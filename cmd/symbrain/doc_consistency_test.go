package main

import (
	"bytes"
	"os"
	"path/filepath"
	"runtime"
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

func TestMemoryHelpMatchesREADME(t *testing.T) {
	var stdout, stderr bytes.Buffer
	if code := run([]string{"memory", "--help"}, &stdout, &stderr); code != exitcodes.ExitOK {
		t.Fatalf("memory help exit code = %d, stderr: %s", code, stderr.String())
	}
	for _, subcommand := range []string{"list", "search", "sync"} {
		if !strings.Contains(stdout.String(), "  "+subcommand) {
			t.Errorf("memory help does not advertise %q:\n%s", subcommand, stdout.String())
		}
	}

	_, filename, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("runtime.Caller failed")
	}
	readme, err := os.ReadFile(filepath.Join(filepath.Dir(filename), "..", "..", "README.md"))
	if err != nil {
		t.Fatalf("read README: %v", err)
	}
	readmeText := strings.ReplaceAll(string(readme), "\r\n", "\n")
	for _, invocation := range []string{
		"symbrain memory list",
		"symbrain memory search <query>",
		"symbrain memory sync",
	} {
		if !strings.Contains(readmeText, invocation) {
			t.Errorf("README does not document %q", invocation)
		}
	}
}
