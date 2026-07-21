package profile

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// withHome points $HOME at a fresh temp directory for the duration of the
// test, so xdg.ProfilesDir() never touches the real user config.
func withHome(t *testing.T) string {
	t.Helper()
	home := t.TempDir()
	t.Setenv("HOME", home)
	return home
}

// writeProfile writes contents to <home>/.config/symbrain/profiles/<name>.toml.
func writeProfile(t *testing.T, home, name, contents string) {
	t.Helper()
	dir := filepath.Join(home, ".config", "symbrain", "profiles")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	path := filepath.Join(dir, name+".toml")
	if err := os.WriteFile(path, []byte(contents), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}
}

func TestLoad_ValidFullProfile(t *testing.T) {
	home := withHome(t)
	writeProfile(t, home, "personal", `
[profile]
name        = "personal"
description = "Full access for trusted personal use"

[servers.vault]
enabled = true
mode    = "full"

[servers.memory]
enabled = true
mode    = "read_write"

[servers.skills]
enabled = true

[audit]
enabled = true
`)

	p, err := Load("personal")
	if err != nil {
		t.Fatalf("Load() error = %v, want nil", err)
	}

	if p.Name != "personal" || p.Description != "Full access for trusted personal use" {
		t.Errorf("Name/Description = %q/%q, want personal/...", p.Name, p.Description)
	}
	if !p.Servers.Vault.Enabled || p.Servers.Vault.Mode != VaultModeFull {
		t.Errorf("Servers.Vault = %+v, want enabled=true mode=full", p.Servers.Vault)
	}
	if !p.Servers.Memory.Enabled || p.Servers.Memory.Mode != MemoryModeReadWrite {
		t.Errorf("Servers.Memory = %+v, want enabled=true mode=read_write", p.Servers.Memory)
	}
	if !p.Servers.Skills.Enabled {
		t.Errorf("Servers.Skills.Enabled = false, want true")
	}
	if !p.Audit.Enabled {
		t.Errorf("Audit.Enabled = false, want true")
	}
	if len(p.Warnings) != 0 {
		t.Errorf("Warnings = %v, want none", p.Warnings)
	}
}

func TestLoad_ValidRestrictedProfile(t *testing.T) {
	home := withHome(t)
	writeProfile(t, home, "restricted", `
[profile]
name        = "restricted"
description = "Least-privilege profile"

[servers.vault]
enabled = true
mode    = "request_only"

[servers.memory]
enabled = true
mode    = "read_only"

[servers.skills]
enabled = true

[audit]
enabled = true
`)

	p, err := Load("restricted")
	if err != nil {
		t.Fatalf("Load() error = %v, want nil", err)
	}
	if p.Servers.Vault.Mode != VaultModeRequestOnly {
		t.Errorf("Servers.Vault.Mode = %q, want %q", p.Servers.Vault.Mode, VaultModeRequestOnly)
	}
	if p.Servers.Memory.Mode != MemoryModeReadOnly {
		t.Errorf("Servers.Memory.Mode = %q, want %q", p.Servers.Memory.Mode, MemoryModeReadOnly)
	}
}

func TestLoad_DefaultsWhenServersAndAuditOmitted(t *testing.T) {
	home := withHome(t)
	writeProfile(t, home, "bare", `
[profile]
name = "bare"
`)

	p, err := Load("bare")
	if err != nil {
		t.Fatalf("Load() error = %v, want nil", err)
	}

	if p.Servers.Vault.Enabled || p.Servers.Memory.Enabled || p.Servers.Skills.Enabled {
		t.Errorf("Servers = %+v, want all disabled by default", p.Servers)
	}
	if !p.Audit.Enabled {
		t.Errorf("Audit.Enabled = false, want true (default)")
	}
}

func TestLoad_ServerEnabledWithoutModeGetsLeastPrivilegeDefault(t *testing.T) {
	home := withHome(t)
	writeProfile(t, home, "no-mode", `
[profile]
name = "no-mode"

[servers.vault]
enabled = true

[servers.memory]
enabled = true
`)

	p, err := Load("no-mode")
	if err != nil {
		t.Fatalf("Load() error = %v, want nil", err)
	}
	if p.Servers.Vault.Mode != VaultModeRequestOnly {
		t.Errorf("Servers.Vault.Mode = %q, want default %q", p.Servers.Vault.Mode, VaultModeRequestOnly)
	}
	if p.Servers.Memory.Mode != MemoryModeReadOnly {
		t.Errorf("Servers.Memory.Mode = %q, want default %q", p.Servers.Memory.Mode, MemoryModeReadOnly)
	}
}

func TestLoad_ToolsAllowAndDenyBothParse(t *testing.T) {
	// internal/policy owns deny-wins *resolution*; this only asserts that
	// both lists round-trip through the schema so policy has something to
	// resolve against — see internal/policy for the full allow/deny matrix.
	home := withHome(t)
	writeProfile(t, home, "lists", `
[profile]
name = "lists"

[servers.memory]
enabled     = true
mode        = "read_write"
tools_allow = ["memory_search", "memory_set"]
tools_deny  = ["memory_set"]
`)

	p, err := Load("lists")
	if err != nil {
		t.Fatalf("Load() error = %v, want nil", err)
	}
	allow := p.Servers.Memory.ToolsAllow
	deny := p.Servers.Memory.ToolsDeny
	if len(allow) != 2 || allow[0] != "memory_search" || allow[1] != "memory_set" {
		t.Errorf("ToolsAllow = %v, want [memory_search memory_set]", allow)
	}
	if len(deny) != 1 || deny[0] != "memory_set" {
		t.Errorf("ToolsDeny = %v, want [memory_set]", deny)
	}
}

func TestLoad_UnknownTopLevelKeyWarnsNotFails(t *testing.T) {
	home := withHome(t)
	writeProfile(t, home, "warny", `
[profile]
name   = "warny"
author = "someone"

[servers.vault]
enabled     = true
mode        = "full"
rate_limit  = 5
`)

	p, err := Load("warny")
	if err != nil {
		t.Fatalf("Load() error = %v, want nil (unknown keys should warn, not fail)", err)
	}
	if len(p.Warnings) == 0 {
		t.Error("Warnings is empty, want warnings about profile.author and servers.vault.rate_limit")
	}
}

func TestLoad_UnknownServerAliasErrors(t *testing.T) {
	home := withHome(t)
	writeProfile(t, home, "bad-alias", `
[profile]
name = "bad-alias"

[servers.wat]
enabled = true
`)

	_, err := Load("bad-alias")
	if err == nil {
		t.Fatal("Load() error = nil, want error for unknown server alias")
	}
	if got := exitcodes.ExitCodeFromError(err); got != exitcodes.ExitNoInput {
		t.Errorf("ExitCodeFromError(err) = %d, want %d", got, exitcodes.ExitNoInput)
	}
}

func TestLoad_NameMismatchErrors(t *testing.T) {
	home := withHome(t)
	writeProfile(t, home, "on-disk-name", `
[profile]
name = "different-name"
`)

	_, err := Load("on-disk-name")
	if err == nil {
		t.Fatal("Load() error = nil, want error for name/filename mismatch")
	}
	if got := exitcodes.ExitCodeFromError(err); got != exitcodes.ExitNoInput {
		t.Errorf("ExitCodeFromError(err) = %d, want %d", got, exitcodes.ExitNoInput)
	}
}

func TestLoad_InvalidVaultModeErrors(t *testing.T) {
	home := withHome(t)
	writeProfile(t, home, "bad-vault-mode", `
[profile]
name = "bad-vault-mode"

[servers.vault]
enabled = true
mode    = "godmode"
`)

	_, err := Load("bad-vault-mode")
	if err == nil {
		t.Fatal("Load() error = nil, want error for invalid vault mode")
	}
	if got := exitcodes.ExitCodeFromError(err); got != exitcodes.ExitNoInput {
		t.Errorf("ExitCodeFromError(err) = %d, want %d", got, exitcodes.ExitNoInput)
	}
}

func TestLoad_InvalidMemoryModeErrors(t *testing.T) {
	home := withHome(t)
	writeProfile(t, home, "bad-memory-mode", `
[profile]
name = "bad-memory-mode"

[servers.memory]
enabled = true
mode    = "write_only"
`)

	_, err := Load("bad-memory-mode")
	if err == nil {
		t.Fatal("Load() error = nil, want error for invalid memory mode")
	}
	if got := exitcodes.ExitCodeFromError(err); got != exitcodes.ExitNoInput {
		t.Errorf("ExitCodeFromError(err) = %d, want %d", got, exitcodes.ExitNoInput)
	}
}

func TestLoad_SkillsModeIsIgnoredWithWarning(t *testing.T) {
	home := withHome(t)
	writeProfile(t, home, "skills-mode", `
[profile]
name = "skills-mode"

[servers.skills]
enabled = true
mode    = "full"
`)

	p, err := Load("skills-mode")
	if err != nil {
		t.Fatalf("Load() error = %v, want nil", err)
	}
	if !p.Servers.Skills.Enabled {
		t.Error("Servers.Skills.Enabled = false, want true")
	}
	if len(p.Warnings) == 0 {
		t.Error("Warnings is empty, want a warning about servers.skills.mode being ignored")
	}
}

func TestLoad_MalformedTOMLErrors(t *testing.T) {
	home := withHome(t)
	writeProfile(t, home, "malformed", `[profile
name = "malformed"`)

	_, err := Load("malformed")
	if err == nil {
		t.Fatal("Load() error = nil, want a parse error")
	}
	if got := exitcodes.ExitCodeFromError(err); got != exitcodes.ExitNoInput {
		t.Errorf("ExitCodeFromError(err) = %d, want %d", got, exitcodes.ExitNoInput)
	}
	if exitcodes.FormatCLIError(err) == "" {
		t.Error("FormatCLIError(err) is empty, want a clear message")
	}
}

func TestLoad_MissingFileErrors(t *testing.T) {
	withHome(t)

	_, err := Load("does-not-exist")
	if err == nil {
		t.Fatal("Load() error = nil, want error for missing profile file")
	}
	if got := exitcodes.ExitCodeFromError(err); got != exitcodes.ExitNoInput {
		t.Errorf("ExitCodeFromError(err) = %d, want %d", got, exitcodes.ExitNoInput)
	}
}

func TestListNames(t *testing.T) {
	home := withHome(t)

	names, err := ListNames()
	if err != nil {
		t.Fatalf("ListNames() with no profiles dir error = %v, want nil", err)
	}
	if len(names) != 0 {
		t.Errorf("ListNames() = %v, want empty", names)
	}

	writeProfile(t, home, "zeta", `[profile]
name = "zeta"`)
	writeProfile(t, home, "alpha", `[profile]
name = "alpha"`)

	names, err = ListNames()
	if err != nil {
		t.Fatalf("ListNames() error = %v, want nil", err)
	}
	want := []string{"alpha", "zeta"}
	if len(names) != len(want) || names[0] != want[0] || names[1] != want[1] {
		t.Errorf("ListNames() = %v, want %v", names, want)
	}
}

func TestLoadAll_ReportsPerFileErrorsWithoutFailingOverall(t *testing.T) {
	home := withHome(t)
	writeProfile(t, home, "good", `[profile]
name = "good"`)
	writeProfile(t, home, "broken", `[profile]
name = "wrong-name"`)

	results, err := LoadAll()
	if err != nil {
		t.Fatalf("LoadAll() error = %v, want nil", err)
	}
	if len(results) != 2 {
		t.Fatalf("len(results) = %d, want 2", len(results))
	}

	byName := map[string]LoadResult{}
	for _, r := range results {
		byName[r.Name] = r
	}

	good, ok := byName["good"]
	if !ok || good.Err != nil || good.Profile == nil {
		t.Errorf("results[good] = %+v, want a successfully loaded profile", good)
	}
	broken, ok := byName["broken"]
	if !ok || broken.Err == nil {
		t.Errorf("results[broken] = %+v, want a name-mismatch error", broken)
	}
}

func TestValidateName(t *testing.T) {
	tests := []struct {
		name string
		want bool // true = valid
	}{
		{"cursor-arbeit", true},
		{"personal", true},
		{"restricted_2", true},
		{"", false},
		{"..", false},
		{"../../etc/passwd", false},
		{"has/slash", false},
		{`has\backslash`, false},
		{"has space", false},
		{`has"quote`, false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := ValidateName(tt.name)
			if tt.want && err != nil {
				t.Errorf("ValidateName(%q) = %v, want nil", tt.name, err)
			}
			if !tt.want && err == nil {
				t.Errorf("ValidateName(%q) = nil, want an error", tt.name)
			}
		})
	}
}

func TestLoad_PathTraversalNameRejected(t *testing.T) {
	withHome(t)

	_, err := Load("../../etc/passwd")
	if err == nil {
		t.Fatal("Load() error = nil, want error for a path-traversal name")
	}
	if got := exitcodes.ExitCodeFromError(err); got != exitcodes.ExitNoInput {
		t.Errorf("ExitCodeFromError(err) = %d, want %d", got, exitcodes.ExitNoInput)
	}
}

func TestExists(t *testing.T) {
	home := withHome(t)
	if Exists("ghost") {
		t.Error("Exists(ghost) = true, want false")
	}
	writeProfile(t, home, "ghost", `[profile]
name = "ghost"`)
	if !Exists("ghost") {
		t.Error("Exists(ghost) = false, want true")
	}
}
