package config

import (
	"os"
	"path/filepath"
	"testing"
)

// writeTempConfig creates a temporary TOML file and returns its path.
func writeTempConfig(t *testing.T, content string) string {
	t.Helper()
	dir := t.TempDir()
	path := filepath.Join(dir, "config.toml")
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("writeTempConfig: %v", err)
	}
	return path
}

func TestDefaultConfig(t *testing.T) {
	cfg := DefaultConfig()

	if cfg.Defaults["shell"] != Ask {
		t.Errorf("Defaults[shell] = %q, want %q", cfg.Defaults["shell"], Ask)
	}
	if cfg.Defaults["read_secret"] != Deny {
		t.Errorf("Defaults[read_secret] = %q, want %q", cfg.Defaults["read_secret"], Deny)
	}
	if cfg.Audit.Path != "symguard-audit.log" {
		t.Errorf("Audit.Path = %q, want %q", cfg.Audit.Path, "symguard-audit.log")
	}
	if len(cfg.Rules) != 0 {
		t.Errorf("Rules len = %d, want 0", len(cfg.Rules))
	}
}

func TestLoad_MissingFile(t *testing.T) {
	path := filepath.Join(t.TempDir(), "nonexistent", "config.toml")
	cfg, err := LoadFrom(path)
	if err != nil {
		t.Fatalf("LoadFrom missing file: unexpected error: %v", err)
	}
	// Should return sensible defaults.
	if cfg.Defaults["shell"] != Ask {
		t.Errorf("Defaults[shell] = %q, want %q", cfg.Defaults["shell"], Ask)
	}
}

func TestLoad_InvalidTOML(t *testing.T) {
	path := writeTempConfig(t, `this is not valid TOML [[[`)
	_, err := LoadFrom(path)
	if err == nil {
		t.Fatal("LoadFrom invalid TOML: expected error, got nil")
	}
}

func TestLoad_InvalidDecision(t *testing.T) {
	path := writeTempConfig(t, `
[defaults]
shell = "bogus"
`)
	_, err := LoadFrom(path)
	if err == nil {
		t.Fatal("LoadFrom invalid decision: expected error, got nil")
	}
}

func TestLoad_EmptyRuleMatch(t *testing.T) {
	path := writeTempConfig(t, `
[[rules]]
decision = "allow"
`)
	_, err := LoadFrom(path)
	if err == nil {
		t.Fatal("LoadFrom empty rule match: expected error, got nil")
	}
}

func TestLoad_FullConfig(t *testing.T) {
	path := writeTempConfig(t, `
[defaults]
shell = "ask"
read_secret = "deny"
write_file = "allow"
network = "ask"

[[rules]]
match.server = "symmemory"
match.tool = "memory_search"
decision = "allow"

[[rules]]
match.server = "symvault"
match.capability = "read_secret"
decision = "ask"

[[rules]]
match.command_contains = ["rm -rf", "curl | sh"]
decision = "deny"

[proxy]
upstream = "http://localhost:3000"

[audit]
path = "/var/log/symguard/audit.log"
encrypt = true
encrypt_age = "age1..."

[[remote]]
name = "dev-server"
provider = "ssh"
host = "10.0.0.1"
allowed_servers = ["filesystem", "shell"]
trust_level = "high"
labels = ["dev", "trusted"]
`)

	cfg, err := LoadFrom(path)
	if err != nil {
		t.Fatalf("LoadFrom full config: %v", err)
	}

	// Defaults
	if cfg.Defaults["shell"] != Ask {
		t.Errorf("Defaults[shell] = %q, want %q", cfg.Defaults["shell"], Ask)
	}
	if cfg.Defaults["read_secret"] != Deny {
		t.Errorf("Defaults[read_secret] = %q, want %q", cfg.Defaults["read_secret"], Deny)
	}
	if cfg.Defaults["write_file"] != Allow {
		t.Errorf("Defaults[write_file] = %q, want %q", cfg.Defaults["write_file"], Allow)
	}

	// Rules
	if len(cfg.Rules) != 3 {
		t.Fatalf("Rules len = %d, want 3", len(cfg.Rules))
	}
	if cfg.Rules[0].Match.Server != "symmemory" || cfg.Rules[0].Match.Tool != "memory_search" {
		t.Errorf("Rules[0].Match = %+v, want server=symmemory tool=memory_search", cfg.Rules[0].Match)
	}
	if cfg.Rules[0].Decision != Allow {
		t.Errorf("Rules[0].Decision = %q, want %q", cfg.Rules[0].Decision, Allow)
	}
	if cfg.Rules[1].Match.Capability != "read_secret" {
		t.Errorf("Rules[1].Match.Capability = %q, want %q", cfg.Rules[1].Match.Capability, "read_secret")
	}
	if len(cfg.Rules[2].Match.CommandContains) != 2 {
		t.Errorf("Rules[2].Match.CommandContains len = %d, want 2", len(cfg.Rules[2].Match.CommandContains))
	}

	// Proxy
	if cfg.Proxy.Upstream != "http://localhost:3000" {
		t.Errorf("Proxy.Upstream = %q, want %q", cfg.Proxy.Upstream, "http://localhost:3000")
	}

	// Audit
	if cfg.Audit.Path != "/var/log/symguard/audit.log" {
		t.Errorf("Audit.Path = %q, want %q", cfg.Audit.Path, "/var/log/symguard/audit.log")
	}
	if !cfg.Audit.Encrypt {
		t.Error("Audit.Encrypt = false, want true")
	}
	if cfg.Audit.EncryptAge != "age1..." {
		t.Errorf("Audit.EncryptAge = %q, want %q", cfg.Audit.EncryptAge, "age1...")
	}

	// Remote
	if len(cfg.Remote) != 1 {
		t.Fatalf("Remote len = %d, want 1", len(cfg.Remote))
	}
	if cfg.Remote[0].Name != "dev-server" {
		t.Errorf("Remote[0].Name = %q, want %q", cfg.Remote[0].Name, "dev-server")
	}
	if cfg.Remote[0].Provider != "ssh" {
		t.Errorf("Remote[0].Provider = %q, want %q", cfg.Remote[0].Provider, "ssh")
	}
	if len(cfg.Remote[0].AllowedServers) != 2 {
		t.Errorf("Remote[0].AllowedServers len = %d, want 2", len(cfg.Remote[0].AllowedServers))
	}
}

func TestConfigPath_SymguardConfigEnv(t *testing.T) {
	t.Setenv("SYMGUARD_CONFIG", "/custom/path/config.toml")
	got := ConfigPath()
	if got != "/custom/path/config.toml" {
		t.Errorf("ConfigPath() = %q, want %q", got, "/custom/path/config.toml")
	}
}

func TestConfigPath_XDGConfigHome(t *testing.T) {
	t.Setenv("SYMGUARD_CONFIG", "")
	t.Setenv("XDG_CONFIG_HOME", "/xdg/config")
	got := ConfigPath()
	want := filepath.Join("/xdg/config", "symguard", "config.toml")
	if got != want {
		t.Errorf("ConfigPath() = %q, want %q", got, want)
	}
}

func TestConfigPath_HomeFallback(t *testing.T) {
	t.Setenv("SYMGUARD_CONFIG", "")
	t.Setenv("XDG_CONFIG_HOME", "")
	home, err := os.UserHomeDir()
	if err != nil {
		t.Skipf("cannot determine home dir: %v", err)
	}
	got := ConfigPath()
	want := filepath.Join(home, ".config", "symguard", "config.toml")
	if got != want {
		t.Errorf("ConfigPath() = %q, want %q", got, want)
	}
}

func TestLoad_AllDecisionsValid(t *testing.T) {
	decisions := []Decision{Allow, Ask, Deny, Redact, ReadOnly, Sandbox}
	for _, d := range decisions {
		path := writeTempConfig(t, `
[defaults]
shell = "`+string(d)+`"
`)
		cfg, err := LoadFrom(path)
		if err != nil {
			t.Errorf("LoadFrom with decision %q: unexpected error: %v", d, err)
			continue
		}
		if cfg.Defaults["shell"] != d {
			t.Errorf("Defaults[shell] = %q, want %q", cfg.Defaults["shell"], d)
		}
	}
}

// ---------------------------------------------------------------------------
// Tests: Load() — integration-style wrapper coverage
// ---------------------------------------------------------------------------

func TestLoad_MissingFileReturnsDefaults(t *testing.T) {
	t.Setenv("SYMGUARD_CONFIG", filepath.Join(t.TempDir(), "nonexistent.toml"))
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() with missing file: unexpected error: %v", err)
	}
	if cfg.Defaults["shell"] != Ask {
		t.Errorf("Defaults[shell] = %q, want %q", cfg.Defaults["shell"], Ask)
	}
}

func TestLoad_ValidConfig(t *testing.T) {
	path := writeTempConfig(t, `
[defaults]
shell = "deny"
read_secret = "allow"
`)
	t.Setenv("SYMGUARD_CONFIG", path)
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() with valid config: unexpected error: %v", err)
	}
	if cfg.Defaults["shell"] != Deny {
		t.Errorf("Defaults[shell] = %q, want %q", cfg.Defaults["shell"], Deny)
	}
	if cfg.Defaults["read_secret"] != Allow {
		t.Errorf("Defaults[read_secret] = %q, want %q", cfg.Defaults["read_secret"], Allow)
	}
}

func TestLoad_OS_InvalidTOML(t *testing.T) {
	path := writeTempConfig(t, `this is not valid TOML [[[`)
	t.Setenv("SYMGUARD_CONFIG", path)
	_, err := Load()
	if err == nil {
		t.Fatal("Load() with invalid TOML: expected error, got nil")
	}
}
