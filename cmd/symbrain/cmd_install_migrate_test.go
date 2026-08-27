package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// writeCursorConfig writes a .cursor/mcp.json with the given server entries
// and returns its path.
func writeCursorConfig(t *testing.T, home string, servers map[string]map[string]any) string {
	t.Helper()
	path := filepath.Join(home, ".cursor", "mcp.json")
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	body := map[string]any{"mcpServers": servers}
	data, err := json.MarshalIndent(body, "", "  ")
	if err != nil {
		t.Fatalf("Marshal: %v", err)
	}
	if err := os.WriteFile(path, data, 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}
	return path
}

// readCursorServerNames reads the mcpServers keys back from a cursor config.
func readCursorServerNames(t *testing.T, path string) map[string]bool {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	var body struct {
		MCPServers map[string]any `json:"mcpServers"`
	}
	if err := json.Unmarshal(data, &body); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	names := make(map[string]bool, len(body.MCPServers))
	for name := range body.MCPServers {
		names[name] = true
	}
	return names
}

func TestCmdInstall_MigratesSupersededCoreEntries(t *testing.T) {
	// install into a harness that still lists symmemory/symskills: those
	// entries are removed (served in-process now), vault is left alone, and
	// symbrain is added.
	home := harnessSandbox(t)
	path := writeCursorConfig(t, home, map[string]map[string]any{
		"symmemory": {"command": "/opt/homebrew/bin/symmemory", "args": []any{"serve"}},
		"symskills": {"command": "symskills", "args": []any{"mcp"}},
		"symvault":  {"command": "symvault", "args": []any{"serve"}},
	})

	var stdout, stderr bytes.Buffer
	code := cmdInstall([]string{"--harness", "cursor", "--profile", "personal"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	names := readCursorServerNames(t, path)
	if names["symmemory"] || names["symskills"] {
		t.Errorf("superseded entries still present: %v", names)
	}
	if !names["symvault"] {
		t.Error("symvault must be left alone")
	}
	if !names["symbrain"] {
		t.Error("symbrain entry missing after install")
	}
	if !strings.Contains(stdout.String(), "migrated superseded core entries") {
		t.Errorf("stdout = %q, want it to report the migration", stdout.String())
	}
}

func TestCmdInstall_MigratesManagedRuntimePathForm(t *testing.T) {
	// The managed-runtime path form (~/.symaira/bin/symmemory) must be
	// matched too — basename matching, not exact path.
	home := harnessSandbox(t)
	path := writeCursorConfig(t, home, map[string]map[string]any{
		"memory": {"command": "/Users/daniel/.symaira/bin/symmemory", "args": []any{"serve", "--stdio"}},
		"vault":  {"command": "symvault", "args": []any{"serve"}},
	})

	var stdout, stderr bytes.Buffer
	code := cmdInstall([]string{"--harness", "cursor", "--profile", "personal"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	names := readCursorServerNames(t, path)
	if names["memory"] {
		t.Errorf("managed-path symmemory entry still present: %v", names)
	}
	if !names["vault"] {
		t.Error("vault server entry must be left alone")
	}
}

func TestCmdInstall_DryRun_ShowsMigrationAndWritesNothing(t *testing.T) {
	home := harnessSandbox(t)
	path := writeCursorConfig(t, home, map[string]map[string]any{
		"symmemory": {"command": "symmemory", "args": []any{"serve"}},
	})

	before, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}

	var stdout, stderr bytes.Buffer
	code := cmdInstall([]string{"--harness", "cursor", "--profile", "personal", "--dry-run"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	after, _ := os.ReadFile(path)
	if string(before) != string(after) {
		t.Error("dry-run must not modify the file")
	}
	if !strings.Contains(stdout.String(), "symmemory") {
		t.Errorf("stdout = %q, want the diff to show the symmemory removal", stdout.String())
	}
	if !strings.Contains(stdout.String(), "migrated superseded core entries") {
		t.Errorf("stdout = %q, want it to report the migration", stdout.String())
	}
}

func TestCmdInstall_KeepSupersededLeavesEntries(t *testing.T) {
	home := harnessSandbox(t)
	path := writeCursorConfig(t, home, map[string]map[string]any{
		"symmemory": {"command": "symmemory", "args": []any{"serve"}},
	})

	var stdout, stderr bytes.Buffer
	code := cmdInstall([]string{"--harness", "cursor", "--profile", "personal", "--keep-superseded"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	names := readCursorServerNames(t, path)
	if !names["symmemory"] {
		t.Error("--keep-superseded should leave the symmemory entry in place")
	}
	if !names["symbrain"] {
		t.Error("symbrain entry missing after install")
	}
}
