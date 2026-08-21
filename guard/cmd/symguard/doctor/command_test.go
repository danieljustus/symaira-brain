package doctor

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-guard/internal/audit"
	"github.com/danieljustus/symaira-guard/internal/config"
)

// hermeticEnv points every host-dependent path (config, discovery, home) at
// fresh temp dirs so Run() sees a deterministic, empty machine.
func hermeticEnv(t *testing.T) {
	t.Helper()
	t.Setenv("HOME", t.TempDir())
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())
	t.Setenv("SYMGUARD_CONFIG", filepath.Join(t.TempDir(), "missing.toml"))
}

// doctorSetVersion pins the version callback for the duration of the test
// and restores the default afterwards.
func doctorSetVersion(t *testing.T, v string) {
	t.Helper()
	SetVersion(func() string { return v })
	t.Cleanup(func() { SetVersion(func() string { return "dev" }) })
}

func TestRun_EmptyAllowlistNoServers(t *testing.T) {
	hermeticEnv(t)
	doctorSetVersion(t, "test-1.2.3")

	var buf strings.Builder
	code := Run(&buf)
	out := buf.String()

	if code != 0 {
		t.Errorf("Run() = %d, want 0 on an empty machine:\n%s", code, out)
	}
	for _, want := range []string{
		"symguard doctor",
		"Version:   test-1.2.3",
		"config           not configured (no config file found)",
		"policy           defaults only (no rules — deny by default)",
		"audit log        not initialized (created on first 'symguard decide')",
		"not configured (empty — deny by default)",
		"none discovered",
		"All basic checks passed",
	} {
		if !strings.Contains(out, want) {
			t.Errorf("Run() output missing %q:\n%s", want, out)
		}
	}
	if strings.Contains(out, "issue(s) found") {
		t.Errorf("Run() reported issues on an empty machine:\n%s", out)
	}
}

func TestRun_DiscoveredServersDenied(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())
	t.Setenv("SYMGUARD_CONFIG", filepath.Join(t.TempDir(), "missing.toml"))

	// One discovered Cursor server; the empty spawn allowlist denies it.
	cursorDir := filepath.Join(home, ".cursor")
	if err := os.MkdirAll(cursorDir, 0o755); err != nil {
		t.Fatalf("MkdirAll() error = %v", err)
	}
	mcp := `{"mcpServers": {"demo": {"command": "/usr/bin/true", "args": ["--once"]}}}`
	if err := os.WriteFile(filepath.Join(cursorDir, "mcp.json"), []byte(mcp), 0o600); err != nil {
		t.Fatalf("WriteFile() error = %v", err)
	}
	doctorSetVersion(t, "test-1.2.3")

	var buf strings.Builder
	code := Run(&buf)
	out := buf.String()

	if code != 1 {
		t.Errorf("Run() = %d, want 1 when a server is denied:\n%s", code, out)
	}
	if !strings.Contains(out, "[DENIED]") {
		t.Errorf("Run() output missing DENIED verdict:\n%s", out)
	}
	if !strings.Contains(out, "demo (cursor/stdio)") {
		t.Errorf("Run() output missing discovered server line:\n%s", out)
	}
	if !strings.Contains(out, "1 issue(s) found") {
		t.Errorf("Run() output missing issue summary:\n%s", out)
	}
	if strings.Contains(out, "All basic checks passed") {
		t.Errorf("Run() reported all-clear despite a denied server:\n%s", out)
	}
}

func TestRun_ConfigError(t *testing.T) {
	hermeticEnv(t)
	doctorSetVersion(t, "test-1.2.3")

	// Invalid TOML at the resolved config path must surface as a
	// fail-closed error: the config line reports it, policy cannot load,
	// and the run exits non-zero (both failing checks count as issues).
	bad := filepath.Join(t.TempDir(), "bad.toml")
	if err := os.WriteFile(bad, []byte("this is not [valid toml"), 0o600); err != nil {
		t.Fatalf("WriteFile() error = %v", err)
	}
	t.Setenv("SYMGUARD_CONFIG", bad)

	var buf strings.Builder
	code := Run(&buf)
	out := buf.String()

	if code != 1 {
		t.Errorf("Run() = %d, want 1 on a config error:\n%s", code, out)
	}
	if !strings.Contains(out, "config           error:") {
		t.Errorf("Run() output missing config error line:\n%s", out)
	}
	if !strings.Contains(out, "policy           not loaded (config error)") {
		t.Errorf("Run() output missing policy error line:\n%s", out)
	}
	if !strings.Contains(out, "2 issue(s) found") {
		t.Errorf("Run() output missing issue summary:\n%s", out)
	}
	if strings.Contains(out, "All basic checks passed") {
		t.Errorf("Run() reported all-clear despite a config error:\n%s", out)
	}
}

func TestRun_ConfigPresent(t *testing.T) {
	// A valid config file must flip the config line to ok and the policy
	// line to the real rule count instead of the old hardcoded stubs.
	dir := t.TempDir()
	t.Setenv("HOME", dir)
	t.Setenv("XDG_CONFIG_HOME", dir)
	t.Setenv("SYMGUARD_CONFIG", filepath.Join(dir, "config.toml"))
	cfg := "[defaults]\nshell = \"allow\"\nread_secret = \"deny\"\n\n[[rules]]\nmatch.server = \"symmemory\"\nmatch.tool = \"memory_search\"\ndecision = \"allow\"\n"
	if err := os.WriteFile(filepath.Join(dir, "config.toml"), []byte(cfg), 0o600); err != nil {
		t.Fatalf("WriteFile() error = %v", err)
	}
	doctorSetVersion(t, "test-1.2.3")

	var buf strings.Builder
	code := Run(&buf)
	out := buf.String()

	if code != 0 {
		t.Errorf("Run() = %d, want 0 for a healthy config:\n%s", code, out)
	}
	for _, want := range []string{
		"config           ok",
		"policy           ok (1 rule(s))",
		"All basic checks passed",
	} {
		if !strings.Contains(out, want) {
			t.Errorf("Run() output missing %q:\n%s", want, out)
		}
	}
	if strings.Contains(out, "issue(s) found") {
		t.Errorf("Run() reported issues on a healthy config:\n%s", out)
	}
}

func TestRun_AuditLogWithoutAnchor(t *testing.T) {
	// decide's current FileSink writes plain JSONL without a chain anchor
	// (the hash-chained sink is Phase 3 wiring). A log without an anchor is
	// therefore the expected state after any `symguard decide` call and
	// must NOT be reported as an issue — otherwise doctor exits 1 on every
	// real machine that ever produced a decision.
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())
	t.Setenv("SYMGUARD_CONFIG", filepath.Join(t.TempDir(), "missing.toml"))
	t.Setenv("XDG_DATA_HOME", t.TempDir())
	logPath := filepath.Join(os.Getenv("XDG_DATA_HOME"), "symguard", "audit.log")
	if err := os.MkdirAll(filepath.Dir(logPath), 0o700); err != nil {
		t.Fatalf("MkdirAll() error = %v", err)
	}
	if err := os.WriteFile(logPath, []byte("{\"entry\":1}\n"), 0o600); err != nil {
		t.Fatalf("WriteFile() error = %v", err)
	}
	doctorSetVersion(t, "test-1.2.3")

	var buf strings.Builder
	code := Run(&buf)
	out := buf.String()

	if code != 0 {
		t.Errorf("Run() = %d, want 0 for an anchorless JSONL log (expected pre-Phase-3 state):\n%s", code, out)
	}
	if !strings.Contains(out, "chain anchor pending Phase 3 sink") {
		t.Errorf("Run() output missing anchor-pending note:\n%s", out)
	}
	if strings.Contains(out, "issue(s) found") {
		t.Errorf("Run() reported issues for the expected anchorless state:\n%s", out)
	}
}

func TestRun_AuditLogCorruptAnchor(t *testing.T) {
	// An anchor that exists but cannot be parsed breaks truncation
	// detection: doctor must report it as a problem and exit non-zero.
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())
	t.Setenv("SYMGUARD_CONFIG", filepath.Join(t.TempDir(), "missing.toml"))
	t.Setenv("XDG_DATA_HOME", t.TempDir())
	logPath := filepath.Join(os.Getenv("XDG_DATA_HOME"), "symguard", "audit.log")
	if err := os.MkdirAll(filepath.Dir(logPath), 0o700); err != nil {
		t.Fatalf("MkdirAll() error = %v", err)
	}
	if err := os.WriteFile(logPath, []byte("{\"entry\":1}\n"), 0o600); err != nil {
		t.Fatalf("WriteFile() error = %v", err)
	}
	if err := os.WriteFile(logPath+".anchor", []byte("not json"), 0o600); err != nil {
		t.Fatalf("WriteFile() error = %v", err)
	}
	doctorSetVersion(t, "test-1.2.3")

	var buf strings.Builder
	code := Run(&buf)
	out := buf.String()

	if code != 1 {
		t.Errorf("Run() = %d, want 1 when the audit anchor is corrupt:\n%s", code, out)
	}
	if !strings.Contains(out, "audit log        error: anchor") || !strings.Contains(out, "1 issue(s) found") {
		t.Errorf("Run() output missing anchor problem:\n%s", out)
	}
}

func TestRun_AuditLogStatError(t *testing.T) {
	// Trigger the non-IsNotExist stat error branch: the audit log path
	// resolves inside a 0000 directory so os.Stat returns permission
	// denied, which is a distinct error from IsNotExist.
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())
	t.Setenv("SYMGUARD_CONFIG", filepath.Join(t.TempDir(), "missing.toml"))

	// Create the symguard data directory and lock it down.
	dataDir := filepath.Join(config.DataDir())
	if err := os.MkdirAll(dataDir, 0o700); err != nil {
		t.Fatalf("MkdirAll() error = %v", err)
	}
	if err := os.Chmod(dataDir, 0o000); err != nil {
		t.Fatalf("Chmod() error = %v", err)
	}
	t.Cleanup(func() { os.Chmod(dataDir, 0o700) }) //nolint:errcheck

	doctorSetVersion(t, "test-1.2.3")

	var buf strings.Builder
	code := Run(&buf)
	out := buf.String()

	if code != 1 {
		t.Errorf("Run() = %d, want 1 when audit log stat fails:\n%s", code, out)
	}
	if !strings.Contains(out, "audit log        error:") {
		t.Errorf("Run() output missing audit log error:\n%s", out)
	}
	if !strings.Contains(out, "1 issue(s) found") {
		t.Errorf("Run() output missing issue summary:\n%s", out)
	}
}

func TestRun_ValidAnchor(t *testing.T) {
	// A valid chain anchor must be reported as ok, not as an issue.
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())
	t.Setenv("SYMGUARD_CONFIG", filepath.Join(t.TempDir(), "missing.toml"))
	t.Setenv("XDG_DATA_HOME", t.TempDir())

	logPath := filepath.Join(os.Getenv("XDG_DATA_HOME"), "symguard", "audit.log")
	if err := os.MkdirAll(filepath.Dir(logPath), 0o700); err != nil {
		t.Fatalf("MkdirAll() error = %v", err)
	}
	if err := os.WriteFile(logPath, []byte("{\"entry\":1}\n"), 0o600); err != nil {
		t.Fatalf("WriteFile() error = %v", err)
	}

	// Write a valid chain anchor via the audit package.
	anchorPath := audit.DefaultAnchorPath(logPath)
	if err := audit.WriteCheckpoint(anchorPath, "abc123", 42); err != nil {
		t.Fatalf("WriteCheckpoint() error = %v", err)
	}

	doctorSetVersion(t, "test-1.2.3")

	var buf strings.Builder
	code := Run(&buf)
	out := buf.String()

	if code != 0 {
		t.Errorf("Run() = %d, want 0 for a valid anchor:\n%s", code, out)
	}
	if !strings.Contains(out, "ok (hash-chained, anchor present)") {
		t.Errorf("Run() output missing anchor-present note:\n%s", out)
	}
	if strings.Contains(out, "issue(s) found") {
		t.Errorf("Run() reported issues for a valid anchor:\n%s", out)
	}
}

func TestRun_NonEmptyAllowlist(t *testing.T) {
	// A config with a spawn allowlist entry must report the entry count.
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", home)
	t.Setenv("XDG_DATA_HOME", t.TempDir())

	cfg := "[[spawn.allowlist]]\npath = \"/usr/local/bin/true\"\n"
	if err := os.WriteFile(filepath.Join(home, "config.toml"), []byte(cfg), 0o600); err != nil {
		t.Fatalf("WriteFile() error = %v", err)
	}
	t.Setenv("SYMGUARD_CONFIG", filepath.Join(home, "config.toml"))

	doctorSetVersion(t, "test-1.2.3")

	var buf strings.Builder
	code := Run(&buf)
	out := buf.String()

	if code != 0 {
		t.Errorf("Run() = %d, want 0 with a valid allowlist:\n%s", code, out)
	}
	if !strings.Contains(out, "spawn allowlist  ok (1 entries)") {
		t.Errorf("Run() output missing allowlist count:\n%s", out)
	}
}

func TestRun_DiscoveryError(t *testing.T) {
	// A malformed mcp.json must surface as a discovery error.
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("XDG_CONFIG_HOME", home)
	t.Setenv("XDG_DATA_HOME", t.TempDir())

	// Valid config with an allowlist entry so the spawn section is entered.
	cfg := "[[spawn.allowlist]]\npath = \"/usr/local/bin/true\"\n"
	if err := os.WriteFile(filepath.Join(home, "config.toml"), []byte(cfg), 0o600); err != nil {
		t.Fatalf("WriteFile() error = %v", err)
	}
	t.Setenv("SYMGUARD_CONFIG", filepath.Join(home, "config.toml"))

	// Malformed mcp.json for Cursor to trigger a parse error.
	cursorDir := filepath.Join(home, ".cursor")
	if err := os.MkdirAll(cursorDir, 0o755); err != nil {
		t.Fatalf("MkdirAll() error = %v", err)
	}
	if err := os.WriteFile(filepath.Join(cursorDir, "mcp.json"), []byte("not valid json {{{"), 0o600); err != nil {
		t.Fatalf("WriteFile() error = %v", err)
	}

	doctorSetVersion(t, "test-1.2.3")

	var buf strings.Builder
	code := Run(&buf)
	out := buf.String()

	if code != 1 {
		t.Errorf("Run() = %d, want 1 on a discovery error:\n%s", code, out)
	}
	if !strings.Contains(out, "mcp servers      error:") {
		t.Errorf("Run() output missing MCP error line:\n%s", out)
	}
	if !strings.Contains(out, "1 issue(s) found") {
		t.Errorf("Run() output missing issue summary:\n%s", out)
	}
}
