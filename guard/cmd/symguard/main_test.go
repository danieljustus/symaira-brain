package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-guard/cmd/symguard/doctor"
	"github.com/danieljustus/symaira-guard/cmd/symguard/version"
	"github.com/danieljustus/symaira-guard/internal/output"
	"golang.org/x/term"
)

func TestRun_NoArgs(t *testing.T) {
	var buf bytes.Buffer
	code := run(nil, &buf)
	if code != 1 {
		t.Errorf("expected exit code 1, got %d", code)
	}
	out := buf.String()
	if !strings.Contains(out, "symguard") {
		t.Error("expected usage message on no args")
	}
}

func TestRun_Help(t *testing.T) {
	tests := []struct {
		name string
		arg  string
	}{
		{"help", "help"},
		{"--help", "--help"},
		{"-h", "-h"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var buf bytes.Buffer
			code := run([]string{tt.arg}, &buf)
			if code != 0 {
				t.Errorf("expected exit code 0, got %d", code)
			}
			out := buf.String()
			if !strings.Contains(out, "symguard") || !strings.Contains(out, "Commands:") {
				t.Error("expected usage message")
			}
		})
	}
}

func TestRun_UnknownCommand(t *testing.T) {
	var buf bytes.Buffer
	code := run([]string{"bogus"}, &buf)
	if code != 1 {
		t.Errorf("expected exit code 1, got %d", code)
	}
	out := buf.String()
	if !strings.Contains(out, "unknown command: bogus") {
		t.Errorf("expected unknown command error, got: %s", out)
	}
}

func TestRun_Version(t *testing.T) {
	var buf bytes.Buffer
	code := run([]string{"version"}, &buf)
	if code != 0 {
		t.Errorf("expected exit code 0, got %d", code)
	}
	out := buf.String()
	if !strings.Contains(out, "symguard") {
		t.Error("expected version output to contain 'symguard'")
	}
	if !strings.Contains(out, "go") {
		t.Error("expected version output to contain go version")
	}
}

func TestRun_Doctor(t *testing.T) {
	var buf bytes.Buffer
	code := run([]string{"doctor"}, &buf)
	out := buf.String()
	if !strings.Contains(out, "symguard doctor") {
		t.Error("expected doctor header")
	}
	if !strings.Contains(out, "Version:") {
		t.Error("expected version in doctor output")
	}
	// Doctor's verdict depends on the host environment: a machine with
	// discovered MCP servers and no spawn allowlist reports issues and
	// exits 1, a clean machine reports all clear and exits 0. The exit
	// code must be consistent with the printed summary.
	hasIssues := strings.Contains(out, "issue(s) found")
	if hasIssues && code != 1 {
		t.Errorf("expected exit code 1 when doctor reports issues, got %d", code)
	}
	if !hasIssues && code != 0 {
		t.Errorf("expected exit code 0 when doctor reports all clear, got %d", code)
	}
}

func TestRun_GrantsList(t *testing.T) {
	t.Setenv("SYMGUARD_DATA", t.TempDir())
	var buf bytes.Buffer
	code := run([]string{"grants", "list"}, &buf)
	if code != 0 {
		t.Errorf("expected exit code 0, got %d", code)
	}
	out := buf.String()
	if !strings.Contains(out, "No active grants.") {
		t.Errorf("expected empty grants list, got: %s", out)
	}
}

func TestPrintUsage(t *testing.T) {
	var buf bytes.Buffer
	printUsage(&buf)
	out := buf.String()
	if !strings.Contains(out, "symguard") {
		t.Error("expected 'symguard' in usage")
	}
	if !strings.Contains(out, "Commands:") {
		t.Error("expected 'Commands:' in usage")
	}
	if !strings.Contains(out, "version") {
		t.Error("expected 'version' in usage")
	}
	if !strings.Contains(out, "doctor") {
		t.Error("expected 'doctor' in usage")
	}
	if !strings.Contains(out, "decide") {
		t.Error("expected 'decide' in usage")
	}
	if !strings.Contains(out, "grants") {
		t.Error("expected 'grants' in usage")
	}
	if !strings.Contains(out, "scan") {
		t.Error("expected 'scan' in usage")
	}
}

func TestRun_Scan(t *testing.T) {
	// Force table format so the assertion is deterministic regardless of
	// whether the test runner's stdout is a terminal.
	output.SetTerminalCheck(func(*os.File) bool { return true })
	defer output.SetTerminalCheck(func(w *os.File) bool { return term.IsTerminal(int(w.Fd())) })

	var buf bytes.Buffer
	code := run([]string{"scan"}, &buf)
	if code != 0 {
		t.Errorf("expected exit code 0, got %d", code)
	}
	out := buf.String()
	if !strings.Contains(out, "Discovered") || !strings.Contains(out, "MCP server") {
		t.Errorf("expected inventory output, got: %s", out)
	}
}

func TestRun_ScanJSON(t *testing.T) {
	var buf bytes.Buffer
	code := run([]string{"scan", "--format", "json"}, &buf)
	if code != 0 {
		t.Errorf("expected exit code 0, got %d", code)
	}
	out := buf.String()
	if !strings.Contains(out, `"servers"`) {
		t.Errorf("expected JSON inventory, got: %s", out)
	}
}

func TestCmdVersion(t *testing.T) {
	var buf bytes.Buffer
	version.Run(nil, &buf)
	out := buf.String()
	if !strings.Contains(out, "symguard") {
		t.Error("expected 'symguard' in version output")
	}
	if !strings.Contains(out, "go") {
		t.Error("expected go version in output")
	}
}

func TestCmdDoctor(t *testing.T) {
	var buf bytes.Buffer
	code := doctor.Run(&buf)
	out := buf.String()
	if !strings.Contains(out, "symguard doctor") {
		t.Error("expected doctor header")
	}
	if !strings.Contains(out, "binary") {
		t.Error("expected 'binary' check")
	}
	if !strings.Contains(out, "config") {
		t.Error("expected 'config' check")
	}
	// Host-dependent verdict: the exit code must match the printed summary.
	hasIssues := strings.Contains(out, "issue(s) found")
	if hasIssues && code != 1 {
		t.Errorf("expected exit code 1 when doctor reports issues, got %d", code)
	}
	if !hasIssues && code != 0 {
		t.Errorf("expected exit code 0 when doctor reports all clear, got %d", code)
	}
}

func TestRun_DoctorExitCodePropagated(t *testing.T) {
	// A discovered server denied by the empty spawn allowlist makes doctor
	// report an issue; the router must propagate the non-zero exit code
	// (it previously always exited 0).
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

	var buf bytes.Buffer
	code := run([]string{"doctor"}, &buf)
	if code != 1 {
		t.Errorf("expected exit code 1 when doctor reports issues, got %d", code)
	}
	if !strings.Contains(buf.String(), "1 issue(s) found") {
		t.Errorf("expected doctor issue summary, got: %s", buf.String())
	}
}

func TestRun_DecideHelp(t *testing.T) {
	var buf bytes.Buffer
	code := run([]string{"decide", "--help"}, &buf)
	if code != 0 {
		t.Errorf("expected exit code 0, got %d", code)
	}
	if !strings.Contains(buf.String(), "symguard decide") {
		t.Error("expected decide usage output")
	}
}

// errWriter fails every Write, simulating a broken decision transport.
type errWriter struct{}

func (errWriter) Write([]byte) (int, error) {
	return 0, os.ErrClosed
}

func TestRun_DecideExitCodePropagated(t *testing.T) {
	// decide's contract: exit 1 when the JSON response itself cannot be
	// written. The router must propagate that code (it was previously
	// discarded). The audit sink is redirected into a temp dir so the
	// test never touches the real XDG data directory.
	t.Setenv("XDG_DATA_HOME", t.TempDir())
	code := run([]string{"decide"}, errWriter{})
	if code != 1 {
		t.Errorf("expected exit code 1 on unwritable response, got %d", code)
	}
}

func TestBuildTime(t *testing.T) {
	// buildTime is an unexported function in the version package.
	// Test that version output includes the placeholder.
	var buf bytes.Buffer
	version.Run(nil, &buf)
	out := buf.String()
	if !strings.Contains(out, "compile-time placeholder") {
		t.Errorf("expected placeholder in version output, got: %s", out)
	}
}

func TestRun_VersionJSON(t *testing.T) {
	var buf bytes.Buffer
	code := run([]string{"version", "--json"}, &buf)
	if code != 0 {
		t.Errorf("expected exit code 0, got %d", code)
	}
	out := buf.String()
	if !strings.Contains(out, `"tool"`) {
		t.Error("expected JSON output with 'tool' field")
	}
	if !strings.Contains(out, `"version"`) {
		t.Error("expected JSON output with 'version' field")
	}
	if !strings.Contains(out, `"schema_version"`) {
		t.Error("expected JSON output with 'schema_version' field")
	}
}

func TestCmdVersionJSON(t *testing.T) {
	var buf bytes.Buffer
	version.Run([]string{"--json"}, &buf)
	out := buf.String()
	if !strings.Contains(out, `"tool":"symguard"`) {
		t.Errorf("expected JSON tool field, got: %s", out)
	}
	if !strings.Contains(out, `"schema_version":1`) {
		t.Errorf("expected schema_version 1, got: %s", out)
	}
}

func TestCmdVersionPlainIsStringFormat(t *testing.T) {
	var buf bytes.Buffer
	version.Run(nil, &buf)
	out := buf.String()
	// Info.String() returns "tool vX.Y.Z"
	if !strings.Contains(out, "symguard") {
		t.Error("expected 'symguard' in output")
	}
	if !strings.Contains(out, "go") {
		t.Error("expected go version in output")
	}
}
