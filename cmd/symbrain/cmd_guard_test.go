package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"

	guardversion "github.com/danieljustus/symaira-brain/guard/cmd/symguard/version"
)

// These tests verify that `symbrain guard <verb>` dispatches to the absorbed
// symguard command implementations with the same behavior (incl. exit codes)
// as the retired standalone binary (ADR 0001, D6).

func TestCmdGuard_NoArgs(t *testing.T) {
	var out, errOut bytes.Buffer
	code := cmdGuard(nil, &out, &errOut)
	// Usage errors follow symbrain's exit-code convention (2), not the
	// retired standalone binary's 1.
	if code != 2 {
		t.Errorf("expected exit code 2, got %d", code)
	}
	if !strings.Contains(out.String(), "symguard") {
		t.Error("expected usage message on no args")
	}
}

func TestCmdGuard_Help(t *testing.T) {
	for _, arg := range []string{"help", "--help", "-h"} {
		t.Run(arg, func(t *testing.T) {
			var out, errOut bytes.Buffer
			code := cmdGuard([]string{arg}, &out, &errOut)
			if code != 0 {
				t.Errorf("expected exit code 0, got %d", code)
			}
			if !strings.Contains(out.String(), "symguard") || !strings.Contains(out.String(), "Commands:") {
				t.Error("expected usage message")
			}
		})
	}
}

func TestCmdGuard_UnknownCommand(t *testing.T) {
	var out, errOut bytes.Buffer
	code := cmdGuard([]string{"bogus"}, &out, &errOut)
	if code != 2 {
		t.Errorf("expected exit code 2, got %d", code)
	}
	if !strings.Contains(errOut.String(), "symbrain guard: unknown command") {
		t.Errorf("expected unknown command error, got: %s", errOut.String())
	}
}

func TestCmdGuard_Version(t *testing.T) {
	var out, errOut bytes.Buffer
	code := cmdGuard([]string{"version"}, &out, &errOut)
	if code != 0 {
		t.Errorf("expected exit code 0, got %d", code)
	}
	if !strings.Contains(out.String(), "symguard") {
		t.Error("expected version output to contain 'symguard'")
	}
	if !strings.Contains(out.String(), "go") {
		t.Error("expected version output to contain go version")
	}
}

func TestCmdGuard_VersionJSON(t *testing.T) {
	var out, errOut bytes.Buffer
	code := cmdGuard([]string{"version", "--json"}, &out, &errOut)
	if code != 0 {
		t.Errorf("expected exit code 0, got %d", code)
	}
	if !strings.Contains(out.String(), `"tool"`) || !strings.Contains(out.String(), `"version"`) {
		t.Error("expected JSON output with tool and version fields")
	}
}

func TestCmdGuard_Doctor(t *testing.T) {
	var out, errOut bytes.Buffer
	code := cmdGuard([]string{"doctor"}, &out, &errOut)
	outStr := out.String()
	if !strings.Contains(outStr, "symguard doctor") {
		t.Error("expected doctor header")
	}
	if !strings.Contains(outStr, "Version:") {
		t.Error("expected version in doctor output")
	}
	// Doctor's verdict depends on the host environment: the exit code must be
	// consistent with the printed summary.
	hasIssues := strings.Contains(outStr, "issue(s) found")
	if hasIssues && code != 1 {
		t.Errorf("expected exit code 1 when doctor reports issues, got %d", code)
	}
	if !hasIssues && code != 0 {
		t.Errorf("expected exit code 0 when doctor reports all clear, got %d", code)
	}
}

func TestCmdGuard_GrantsList(t *testing.T) {
	t.Setenv("SYMGUARD_DATA", t.TempDir())
	var out, errOut bytes.Buffer
	code := cmdGuard([]string{"grants", "list"}, &out, &errOut)
	if code != 0 {
		t.Errorf("expected exit code 0, got %d", code)
	}
	if !strings.Contains(out.String(), "No active grants.") {
		t.Errorf("expected empty grants list, got: %s", out.String())
	}
}

func TestCmdGuard_Scan(t *testing.T) {
	// In the test runner stdout is not a terminal, so scan renders the
	// machine-readable JSON inventory by default (output.Resolve).
	var out, errOut bytes.Buffer
	code := cmdGuard([]string{"scan"}, &out, &errOut)
	if code != 0 {
		t.Errorf("expected exit code 0, got %d", code)
	}
	if !strings.Contains(out.String(), `"servers"`) {
		t.Errorf("expected JSON inventory, got: %s", out.String())
	}
}

func TestCmdGuard_ScanJSON(t *testing.T) {
	var out, errOut bytes.Buffer
	code := cmdGuard([]string{"scan", "--format", "json"}, &out, &errOut)
	if code != 0 {
		t.Errorf("expected exit code 0, got %d", code)
	}
	if !strings.Contains(out.String(), `"servers"`) {
		t.Errorf("expected JSON inventory, got: %s", out.String())
	}
}

func TestCmdGuard_DoctorExitCodePropagated(t *testing.T) {
	// A discovered server denied by the empty spawn allowlist makes doctor
	// report an issue; the dispatch must propagate the non-zero exit code.
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())
	t.Setenv("SYMGUARD_CONFIG", filepath.Join(t.TempDir(), "missing.toml"))

	cursorDir := filepath.Join(home, ".cursor")
	if err := os.MkdirAll(cursorDir, 0o755); err != nil {
		t.Fatalf("MkdirAll() error = %v", err)
	}
	mcp := `{"mcpServers": {"demo": {"command": "/usr/bin/true"}}}`
	if err := os.WriteFile(filepath.Join(cursorDir, "mcp.json"), []byte(mcp), 0o600); err != nil {
		t.Fatalf("WriteFile() error = %v", err)
	}

	var out, errOut bytes.Buffer
	code := cmdGuard([]string{"doctor"}, &out, &errOut)
	if code != 1 {
		t.Errorf("expected exit code 1 when doctor reports issues, got %d", code)
	}
	if !strings.Contains(out.String(), "1 issue(s) found") {
		t.Errorf("expected doctor issue summary, got: %s", out.String())
	}
}

func TestCmdGuard_DecideHelp(t *testing.T) {
	var out, errOut bytes.Buffer
	code := cmdGuard([]string{"decide", "--help"}, &out, &errOut)
	if code != 0 {
		t.Errorf("expected exit code 0, got %d", code)
	}
	if !strings.Contains(out.String(), "symguard decide") {
		t.Error("expected decide usage output")
	}
}

type guardErrWriter struct{}

func (guardErrWriter) Write([]byte) (int, error) {
	return 0, os.ErrClosed
}

func TestCmdGuard_DecideExitCodePropagated(t *testing.T) {
	// decide's contract: exit 1 when the JSON response itself cannot be
	// written. The dispatch must propagate that code. The audit sink is
	// redirected into a temp dir so the test never touches the real XDG
	// data directory.
	t.Setenv("XDG_DATA_HOME", t.TempDir())
	var out bytes.Buffer
	code := cmdGuard([]string{"decide"}, guardErrWriter{}, &out)
	if code != 1 {
		t.Errorf("expected exit code 1 on unwritable response, got %d", code)
	}
}

func TestGuardVersion_Placeholder(t *testing.T) {
	// buildTime is an unexported function in the guard version package.
	// Verify the version output still carries the compile-time placeholder.
	var buf bytes.Buffer
	guardversion.Run(nil, &buf)
	if !strings.Contains(buf.String(), "compile-time placeholder") {
		t.Errorf("expected placeholder in version output, got: %s", buf.String())
	}
}
