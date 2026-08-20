package config

import (
	"os"
	"path/filepath"
	"testing"
)

func writeGlobalConfig(t *testing.T, content string) string {
	t.Helper()
	home := t.TempDir()
	oldHome := os.Getenv("HOME")
	t.Setenv("HOME", home)
	t.Cleanup(func() { _ = os.Setenv("HOME", oldHome) })

	cfgDir := filepath.Join(home, ".config", "symskills")
	if err := os.MkdirAll(cfgDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(cfgDir, "config.toml"), []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
	return home
}

func TestLoadCapabilitiesFromGlobalConfig(t *testing.T) {
	writeGlobalConfig(t, `library_dir = "/tmp/lib"

[capabilities.codex]
subagents = true
mcp = false

[capabilities.opencode]
subagents = false
`)
	caps, err := LoadCapabilities()
	if err != nil {
		t.Fatalf("LoadCapabilities: %v", err)
	}
	if got := caps["codex"]["subagents"]; !got {
		t.Errorf("codex.subagents: got %v, want true", got)
	}
	if got, ok := caps["codex"]["mcp"]; !ok || got {
		t.Errorf("codex.mcp: got %v (declared: %v), want an explicit false", got, ok)
	}
	if got, ok := caps["opencode"]["subagents"]; !ok || got {
		t.Errorf("opencode.subagents: got %v (declared: %v), want an explicit false", got, ok)
	}
	if _, ok := caps["claude"]; ok {
		t.Error("a target the config does not mention must stay undeclared")
	}
}

func TestLoadCapabilitiesMissingFile(t *testing.T) {
	home := t.TempDir()
	oldHome := os.Getenv("HOME")
	t.Setenv("HOME", home)
	t.Cleanup(func() { _ = os.Setenv("HOME", oldHome) })

	caps, err := LoadCapabilities()
	if err != nil {
		t.Fatalf("LoadCapabilities with no config: %v", err)
	}
	if len(caps) != 0 {
		t.Errorf("expected no declarations, got %v", caps)
	}
}

// TestLoadCapabilitiesConfigWithoutCapabilitiesTable guards the common case:
// an existing config that predates this feature must load cleanly.
func TestLoadCapabilitiesConfigWithoutCapabilitiesTable(t *testing.T) {
	writeGlobalConfig(t, "library_dir = \"/tmp/lib\"\n")
	caps, err := LoadCapabilities()
	if err != nil {
		t.Fatalf("LoadCapabilities: %v", err)
	}
	if len(caps) != 0 {
		t.Errorf("expected no declarations, got %v", caps)
	}
}
