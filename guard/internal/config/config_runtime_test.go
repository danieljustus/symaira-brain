package config

import (
	"fmt"
	"os"
	"path/filepath"
	"testing"
)

func TestDefaultConfig_SequenceDisabledByDefault(t *testing.T) {
	cfg := DefaultConfig()
	if cfg.Sequence.Enabled {
		t.Error("sequence must be disabled by default")
	}
	if cfg.Sequence.Threshold != 3 {
		t.Errorf("default sequence threshold = %d, want 3", cfg.Sequence.Threshold)
	}
}

func TestLoad_SequenceEnabledDefaultsThreshold(t *testing.T) {
	path := writeTempConfig(t, `
[sequence]
enabled = true
`)
	cfg, err := LoadFrom(path)
	if err != nil {
		t.Fatalf("LoadFrom: %v", err)
	}
	if !cfg.Sequence.Enabled {
		t.Error("sequence should be enabled")
	}
	if cfg.Sequence.Threshold != 3 {
		t.Errorf("threshold = %d, want default 3", cfg.Sequence.Threshold)
	}
}

func TestLoad_SequenceThresholdZeroDefaults(t *testing.T) {
	path := writeTempConfig(t, `
[sequence]
enabled = true
threshold = 0
`)
	cfg, err := LoadFrom(path)
	if err != nil {
		t.Fatalf("LoadFrom: %v", err)
	}
	if cfg.Sequence.Threshold != 3 {
		t.Errorf("threshold = %d, want default 3", cfg.Sequence.Threshold)
	}
}

func TestLoad_SequenceConfiguredThreshold(t *testing.T) {
	path := writeTempConfig(t, `
[sequence]
enabled = true
threshold = 5
`)
	cfg, err := LoadFrom(path)
	if err != nil {
		t.Fatalf("LoadFrom: %v", err)
	}
	if !cfg.Sequence.Enabled {
		t.Error("sequence should be enabled")
	}
	if cfg.Sequence.Threshold != 5 {
		t.Errorf("threshold = %d, want 5", cfg.Sequence.Threshold)
	}
}

func TestLoad_SequenceInvalidThreshold(t *testing.T) {
	for _, th := range []int{1, -2} {
		path := writeTempConfig(t, fmt.Sprintf(`
[sequence]
enabled = true
threshold = %d
`, th))
		_, err := LoadFrom(path)
		if err == nil {
			t.Errorf("threshold %d: expected error, got nil", th)
		}
	}
}

// ---------------------------------------------------------------------------
// Tests: spawn allowlist
// ---------------------------------------------------------------------------

func TestLoad_SpawnAllowlist(t *testing.T) {
	path := writeTempConfig(t, `
[spawn]

[[spawn.allowlist]]
path = "/usr/local/bin/node"
argv_prefix = ["server.js", "--port"]

[[spawn.allowlist]]
path = "/opt/homebrew/bin/uvx"
`)

	cfg, err := LoadFrom(path)
	if err != nil {
		t.Fatalf("LoadFrom with spawn allowlist: unexpected error: %v", err)
	}
	if len(cfg.Spawn.Allowlist) != 2 {
		t.Fatalf("Spawn.Allowlist len = %d, want 2", len(cfg.Spawn.Allowlist))
	}
	if cfg.Spawn.Allowlist[0].Path != "/usr/local/bin/node" {
		t.Errorf("Allowlist[0].Path = %q, want %q", cfg.Spawn.Allowlist[0].Path, "/usr/local/bin/node")
	}
	if len(cfg.Spawn.Allowlist[0].ArgvPrefix) != 2 || cfg.Spawn.Allowlist[0].ArgvPrefix[1] != "--port" {
		t.Errorf("Allowlist[0].ArgvPrefix = %v, want [server.js --port]", cfg.Spawn.Allowlist[0].ArgvPrefix)
	}
	if cfg.Spawn.Allowlist[1].Path != "/opt/homebrew/bin/uvx" {
		t.Errorf("Allowlist[1].Path = %q, want %q", cfg.Spawn.Allowlist[1].Path, "/opt/homebrew/bin/uvx")
	}
	if len(cfg.Spawn.Allowlist[1].ArgvPrefix) != 0 {
		t.Errorf("Allowlist[1].ArgvPrefix = %v, want empty", cfg.Spawn.Allowlist[1].ArgvPrefix)
	}
}

func TestLoad_SpawnAllowlistDefaultsToEmpty(t *testing.T) {
	// No [spawn] section: the allowlist must be empty (deny by default).
	path := writeTempConfig(t, `
[defaults]
shell = "ask"
`)
	cfg, err := LoadFrom(path)
	if err != nil {
		t.Fatalf("LoadFrom without spawn section: unexpected error: %v", err)
	}
	if len(cfg.Spawn.Allowlist) != 0 {
		t.Errorf("Spawn.Allowlist len = %d, want 0 (deny by default)", len(cfg.Spawn.Allowlist))
	}
}

func TestLoad_SpawnAllowlistValidation(t *testing.T) {
	tests := []struct {
		name    string
		content string
	}{
		{"missing path", "[spawn]\n[[spawn.allowlist]]\nargv_prefix = [\"x\"]\n"},
		{"relative path", "[spawn]\n[[spawn.allowlist]]\npath = \"node\"\n"},
		{"relative path with slashes", "[spawn]\n[[spawn.allowlist]]\npath = \"usr/bin/node\"\n"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			path := writeTempConfig(t, tt.content)
			_, err := LoadFrom(path)
			if err == nil {
				t.Fatalf("LoadFrom with invalid spawn entry: expected error, got nil")
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Tests: DataDir
// ---------------------------------------------------------------------------

func TestDataDir_XDGDataHome(t *testing.T) {
	t.Setenv("XDG_DATA_HOME", "/xdg/data")
	want := filepath.Join("/xdg/data", "symguard")
	if got := DataDir(); got != want {
		t.Errorf("with XDG_DATA_HOME: got %q, want %q", got, want)
	}
}

func TestDataDir_HomeFallback(t *testing.T) {
	t.Setenv("XDG_DATA_HOME", "")
	home, err := os.UserHomeDir()
	if err != nil {
		t.Skipf("cannot determine home dir: %v", err)
	}
	want := filepath.Join(home, ".local", "share", "symguard")
	if got := DataDir(); got != want {
		t.Errorf("without XDG_DATA_HOME: got %q, want %q", got, want)
	}
}

func TestDataDir_TempDirFallback(t *testing.T) {
	t.Setenv("XDG_DATA_HOME", "")
	// Override HOME to trigger UserHomeDir error — on most systems this
	// is hard to force, so we skip if UserHomeDir still succeeds.
	t.Setenv("HOME", "/nonexistent/path/to/home/that/does/not/exist")
	homeErr := func() error {
		_, err := os.UserHomeDir()
		return err
	}()
	if homeErr != nil {
		want := filepath.Join(os.TempDir(), "symguard")
		if got := DataDir(); got != want {
			t.Errorf("on UserHomeDir error: got %q, want %q", got, want)
		}
	} else {
		t.Skip("UserHomeDir did not fail with overridden HOME; cannot test temp fallback")
	}
}

func TestDefaultConfig_SpawnAllowlistEmpty(t *testing.T) {
	cfg := DefaultConfig()
	if len(cfg.Spawn.Allowlist) != 0 {
		t.Errorf("DefaultConfig Spawn.Allowlist len = %d, want 0", len(cfg.Spawn.Allowlist))
	}
}
