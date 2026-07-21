package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/harness"
	"github.com/danieljustus/symaira-corekit/exitcodes"
)

func TestCmdUninstall_MissingHarnessFlag(t *testing.T) {
	harnessSandbox(t)
	var stdout, stderr bytes.Buffer
	code := cmdUninstall(nil, &stdout, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("code = %d, want %d", code, exitcodes.ExitNoInput)
	}
}

func TestCmdUninstall_NoConfigFile(t *testing.T) {
	harnessSandbox(t)
	var stdout, stderr bytes.Buffer
	code := cmdUninstall([]string{"--harness", "claude"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	if !strings.Contains(stdout.String(), "nothing to uninstall") {
		t.Errorf("stdout = %q, want it to say there's nothing to uninstall", stdout.String())
	}
}

func TestCmdUninstall_NotInstalled(t *testing.T) {
	home := harnessSandbox(t)
	path := filepath.Join(home, ".cursor", "mcp.json")
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	if err := os.WriteFile(path, []byte("{\n  \"unrelated\": true\n}\n"), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	var stdout, stderr bytes.Buffer
	code := cmdUninstall([]string{"--harness", "cursor"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}
	if !strings.Contains(stdout.String(), "not installed") {
		t.Errorf("stdout = %q, want it to say symbrain is not installed", stdout.String())
	}

	matches, _ := filepath.Glob(path + ".bak.*")
	if len(matches) != 0 {
		t.Errorf("uninstall backed up a file it made no change to: %v", matches)
	}
}

func TestCmdUninstall_OnlyRemovesSymbrainEntry(t *testing.T) {
	home := harnessSandbox(t)
	path := filepath.Join(home, ".cursor", "mcp.json")
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	content := "{\n  \"mcpServers\": {\n    \"other-tool\": {\n      \"args\": [\n        \"run\"\n      ],\n      \"command\": \"other-tool\"\n    },\n    \"symbrain\": {\n      \"args\": [\n        \"serve\",\n        \"--profile\",\n        \"personal\"\n      ],\n      \"command\": \"symbrain\"\n    }\n  }\n}\n"
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	var stdout, stderr bytes.Buffer
	code := cmdUninstall([]string{"--harness", "cursor"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	h := harnessByName(t, "cursor")
	doc, err := harness.Parse(h, data)
	if err != nil {
		t.Fatalf("harness.Parse: %v", err)
	}
	if _, ok := doc.Server(harness.ServerName); ok {
		t.Error("symbrain entry still present after uninstall")
	}
	other, ok := doc.Server("other-tool")
	if !ok {
		t.Fatal("unrelated server entry was removed by uninstall")
	}
	if other.Command != "other-tool" {
		t.Errorf("other-tool Command = %q, want %q", other.Command, "other-tool")
	}
}

func TestCmdUninstall_DryRun_WritesNothing(t *testing.T) {
	home := harnessSandbox(t)
	path := filepath.Join(home, ".cursor", "mcp.json")
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	content := "{\n  \"mcpServers\": {\n    \"symbrain\": {\n      \"args\": [\n        \"serve\",\n        \"--profile\",\n        \"personal\"\n      ],\n      \"command\": \"symbrain\"\n    }\n  }\n}\n"
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}
	before, err := os.Stat(path)
	if err != nil {
		t.Fatalf("Stat: %v", err)
	}

	var stdout, stderr bytes.Buffer
	code := cmdUninstall([]string{"--harness", "cursor", "--dry-run"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	after, err := os.Stat(path)
	if err != nil {
		t.Fatalf("Stat after dry-run: %v", err)
	}
	if !before.ModTime().Equal(after.ModTime()) {
		t.Errorf("mtime changed by --dry-run: before %v, after %v", before.ModTime(), after.ModTime())
	}
	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	if string(got) != content {
		t.Errorf("content changed by --dry-run:\ngot:  %q\nwant: %q", got, content)
	}
	matches, _ := filepath.Glob(path + ".bak.*")
	if len(matches) != 0 {
		t.Errorf("--dry-run created a backup: %v", matches)
	}
	if !strings.Contains(stdout.String(), "-") {
		t.Errorf("dry-run stdout should contain a diff with removed lines: %q", stdout.String())
	}
}

func TestCmdUninstall_RefusesCorruptConfig(t *testing.T) {
	home := harnessSandbox(t)
	path := filepath.Join(home, ".claude.json")
	if err := os.WriteFile(path, []byte("{not valid json"), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	var stdout, stderr bytes.Buffer
	code := cmdUninstall([]string{"--harness", "claude"}, &stdout, &stderr)
	if code != exitcodes.ExitNoInput {
		t.Fatalf("code = %d, want %d (ExitNoInput) (stderr: %s)", code, exitcodes.ExitNoInput, stderr.String())
	}
}

func TestCmdUninstall_LeavesForeignEntrySharingTheNameAlone(t *testing.T) {
	// A "symbrain" entry whose command does NOT resolve to the symbrain
	// binary (e.g. hand-edited, or a coincidentally-named unrelated tool)
	// must never be touched by uninstall.
	home := harnessSandbox(t)
	path := filepath.Join(home, ".cursor", "mcp.json")
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	content := "{\n  \"mcpServers\": {\n    \"symbrain\": {\n      \"args\": [\n        \"foo\"\n      ],\n      \"command\": \"not-symbrain\"\n    }\n  }\n}\n"
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	var stdout, stderr bytes.Buffer
	code := cmdUninstall([]string{"--harness", "cursor"}, &stdout, &stderr)
	if code != exitcodes.ExitOK {
		t.Fatalf("code = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
	}

	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	if string(got) != content {
		t.Errorf("uninstall modified a foreign entry named \"symbrain\":\ngot:  %q\nwant: %q", got, content)
	}
}

// TestInstallThenUninstall_RoundTripsToOriginalConfig is the acceptance
// test from issue #20: for every registered harness, install followed by
// uninstall must restore the config file to be byte-equivalent to what it
// was before install ever touched it.
func TestInstallThenUninstall_RoundTripsToOriginalConfig(t *testing.T) {
	fixtures := map[harness.Name]string{
		harness.Claude:        "{\n  \"mcpServers\": {\n    \"other-tool\": {\n      \"args\": [\n        \"run\"\n      ],\n      \"command\": \"other-tool\"\n    }\n  },\n  \"userId\": \"u-123\"\n}\n",
		harness.ClaudeDesktop: "{\n  \"theme\": \"dark\"\n}\n",
		harness.Cursor:        "{\n  \"mcpServers\": {\n    \"filesystem\": {\n      \"args\": [\n        \"--root\",\n        \"/tmp\"\n      ],\n      \"command\": \"mcp-filesystem\"\n    }\n  }\n}\n",
		harness.Opencode:      "{}\n",
		harness.Codex:         "model = \"o3\"\n\n[mcp_servers]\n  [mcp_servers.other-tool]\n    args = [\"run\"]\n    command = \"other-tool\"\n",
		harness.Gemini:        "{}\n",
	}

	for _, h := range harness.All {
		t.Run(string(h.Name), func(t *testing.T) {
			harnessSandbox(t)

			path, err := h.ConfigPath()
			if err != nil {
				t.Fatalf("ConfigPath: %v", err)
			}
			original := fixtures[h.Name]
			if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
				t.Fatalf("MkdirAll: %v", err)
			}
			if err := os.WriteFile(path, []byte(original), 0o644); err != nil {
				t.Fatalf("WriteFile: %v", err)
			}

			var stdout, stderr bytes.Buffer
			if code := cmdInstall([]string{"--harness", string(h.Name), "--profile", "personal"}, &stdout, &stderr); code != exitcodes.ExitOK {
				t.Fatalf("cmdInstall() = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
			}

			installed, err := os.ReadFile(path)
			if err != nil {
				t.Fatalf("ReadFile after install: %v", err)
			}
			if string(installed) == original {
				t.Fatal("install did not change the config file")
			}

			stdout.Reset()
			stderr.Reset()
			if code := cmdUninstall([]string{"--harness", string(h.Name)}, &stdout, &stderr); code != exitcodes.ExitOK {
				t.Fatalf("cmdUninstall() = %d, want %d (stderr: %s)", code, exitcodes.ExitOK, stderr.String())
			}

			final, err := os.ReadFile(path)
			if err != nil {
				t.Fatalf("ReadFile after uninstall: %v", err)
			}
			if string(final) != original {
				t.Errorf("round-trip mismatch for %s\n--- got ---\n%s\n--- want (original) ---\n%s", h.Name, final, original)
			}
		})
	}
}
