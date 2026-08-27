package profile

import (
	"os"
	"path/filepath"
	"strings"
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
	if !p.Server(ServerVault).Enabled || p.Server(ServerVault).Mode != VaultModeFull {
		t.Errorf("Servers.Vault = %+v, want enabled=true mode=full", p.Server(ServerVault))
	}
	if !p.Server(ServerMemory).Enabled || p.Server(ServerMemory).Mode != MemoryModeReadWrite {
		t.Errorf("Servers.Memory = %+v, want enabled=true mode=read_write", p.Server(ServerMemory))
	}
	if !p.Server(ServerSkills).Enabled {
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
	if p.Server(ServerVault).Mode != VaultModeRequestOnly {
		t.Errorf("Servers.Vault.Mode = %q, want %q", p.Server(ServerVault).Mode, VaultModeRequestOnly)
	}
	if p.Server(ServerMemory).Mode != MemoryModeReadOnly {
		t.Errorf("Servers.Memory.Mode = %q, want %q", p.Server(ServerMemory).Mode, MemoryModeReadOnly)
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

	if p.Server(ServerVault).Enabled || p.Server(ServerMemory).Enabled || p.Server(ServerSkills).Enabled {
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
	if p.Server(ServerVault).Mode != VaultModeRequestOnly {
		t.Errorf("Servers.Vault.Mode = %q, want default %q", p.Server(ServerVault).Mode, VaultModeRequestOnly)
	}
	if p.Server(ServerMemory).Mode != MemoryModeReadOnly {
		t.Errorf("Servers.Memory.Mode = %q, want default %q", p.Server(ServerMemory).Mode, MemoryModeReadOnly)
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
	allow := p.Server(ServerMemory).ToolsAllow
	deny := p.Server(ServerMemory).ToolsDeny
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

func TestLoad_ForeignServerWithoutTransportErrors(t *testing.T) {
	// An unknown alias is now a foreign server (ADR 0001, D2/D4): it must
	// declare a transport (command or url), otherwise the profile is invalid.
	home := withHome(t)
	writeProfile(t, home, "bad-alias", `
[profile]
name = "bad-alias"

[servers.wat]
enabled = true
`)

	_, err := Load("bad-alias")
	if err == nil {
		t.Fatal("Load() error = nil, want error for foreign server without command/url")
	}
	if !strings.Contains(err.Error(), "requires command") {
		t.Errorf("err = %q, want 'requires command (with optional args) or url'", err)
	}
	if got := exitcodes.ExitCodeFromError(err); got != exitcodes.ExitNoInput {
		t.Errorf("ExitCodeFromError(err) = %d, want %d", got, exitcodes.ExitNoInput)
	}
}

func TestLoad_CoreAliasCannotBeForeign(t *testing.T) {
	// The four core aliases are reserved; redefining one with a foreign
	// transport (command/args/url) is a collision and must be rejected.
	home := withHome(t)
	writeProfile(t, home, "vault-as-foreign", `
[profile]
name = "vault-as-foreign"

[servers.vault]
enabled = true
command = "/usr/bin/some-mcp"
`)

	_, err := Load("vault-as-foreign")
	if err == nil {
		t.Fatal("Load() error = nil, want error for core alias carrying command")
	}
	if !strings.Contains(err.Error(), "not a foreign server") {
		t.Errorf("err = %q, want 'not a foreign server'", err)
	}
}

func TestLoad_ForeignServerWithCommandAndArgs(t *testing.T) {
	home := withHome(t)
	writeProfile(t, home, "with-foreign", `
[profile]
name = "with-foreign"

[servers.zotero]
enabled = true
command = "/usr/local/bin/zotero-mcp"
args = ["--stdio", "--log", "/tmp/z.log"]

[servers.vault]
enabled = true
mode = "full"
`)

	p, err := Load("with-foreign")
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}

	z := p.Server("zotero")
	if !z.Enabled || z.Command != "/usr/local/bin/zotero-mcp" {
		t.Errorf("zotero = %+v, want enabled with command", z)
	}
	if len(z.Args) != 3 || z.Args[0] != "--stdio" || z.Args[1] != "--log" || z.Args[2] != "/tmp/z.log" {
		t.Errorf("zotero args = %v, want [--stdio --log /tmp/z.log]", z.Args)
	}
	if IsCoreAlias("zotero") {
		t.Error("zotero should not be a core alias")
	}

	// The four cores are still present with their defaults.
	if !p.Server(ServerVault).Enabled || p.Server(ServerVault).Mode != VaultModeFull {
		t.Errorf("vault = %+v, want enabled full", p.Server(ServerVault))
	}
	if p.Server(ServerMemory).Enabled || p.Server(ServerUsage).Enabled {
		t.Error("memory/usage should stay disabled when absent")
	}
}

func TestLoad_ForeignServerWithURL(t *testing.T) {
	home := withHome(t)
	writeProfile(t, home, "with-url", `
[profile]
name = "with-url"

[servers.fig]
enabled = true
url = "https://mcp.example.com/sse"
`)

	p, err := Load("with-url")
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	fig := p.Server("fig")
	if !fig.Enabled || fig.URL != "https://mcp.example.com/sse" {
		t.Errorf("fig = %+v, want enabled with url", fig)
	}
}

func TestLoad_ForeignServerModeIgnoredWithWarning(t *testing.T) {
	home := withHome(t)
	writeProfile(t, home, "foreign-mode", `
[profile]
name = "foreign-mode"

[servers.fig]
enabled = true
mode = "full"
command = "/usr/bin/fig-mcp"
`)

	p, err := Load("foreign-mode")
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	found := false
	for _, w := range p.Warnings {
		if strings.Contains(w, "fig") && strings.Contains(w, "mode") {
			found = true
		}
	}
	if !found {
		t.Errorf("Warnings = %v, want a mode-ignored warning for fig", p.Warnings)
	}
}

func TestLoad_ForeignServerAccessValidation(t *testing.T) {
	// Invalid access class is a hard error; "read" is carried; empty
	// defaults to "write" (filter model, issue #335).
	home := withHome(t)
	writeProfile(t, home, "bad-access", `
[profile]
name = "bad-access"

[servers.fig]
enabled = true
command = "/usr/bin/fig-mcp"
access = "exec"
`)
	if _, err := Load("bad-access"); err == nil {
		t.Fatal("Load() error = nil, want error for invalid access class")
	}

	writeProfile(t, home, "read-access", `
[profile]
name = "read-access"

[servers.fig]
enabled = true
command = "/usr/bin/fig-mcp"
access = "read"
`)
	p, err := Load("read-access")
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if got := p.Server("fig").Access; got != "read" {
		t.Errorf("fig access = %q, want %q", got, "read")
	}

	writeProfile(t, home, "default-access", `
[profile]
name = "default-access"

[servers.fig]
enabled = true
command = "/usr/bin/fig-mcp"
`)
	p2, err := Load("default-access")
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if got := p2.Server("fig").Access; got != ForeignAccessWrite {
		t.Errorf("fig access = %q, want default %q", got, ForeignAccessWrite)
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
	if !p.Server(ServerSkills).Enabled {
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

func TestLoadFile_ValidRoomLocalProfile(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "room-profile.toml")
	contents := `[profile]
name        = "room"
description = "Room-local profile"

[servers.vault]
enabled = true
mode    = "request_only"

[servers.memory]
enabled = true
mode    = "read_only"

[servers.skills]
enabled = false
`
	if err := os.WriteFile(path, []byte(contents), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	p, err := LoadFile(path)
	if err != nil {
		t.Fatalf("LoadFile() error = %v, want nil", err)
	}
	if p.Name != "room" || p.Description != "Room-local profile" {
		t.Errorf("Name/Description = %q/%q, want room/...", p.Name, p.Description)
	}
	if !p.Server(ServerVault).Enabled || p.Server(ServerVault).Mode != VaultModeRequestOnly {
		t.Errorf("Servers.Vault = %+v, want enabled=true mode=request_only", p.Server(ServerVault))
	}
	if !p.Server(ServerMemory).Enabled || p.Server(ServerMemory).Mode != MemoryModeReadOnly {
		t.Errorf("Servers.Memory = %+v, want enabled=true mode=read_only", p.Server(ServerMemory))
	}
}

func TestLoadFile_Errors(t *testing.T) {
	dir := t.TempDir()

	t.Run("missing file", func(t *testing.T) {
		_, err := LoadFile(filepath.Join(dir, "nope.toml"))
		if err == nil {
			t.Fatal("LoadFile() error = nil, want error for a missing file")
		}
		if got := exitcodes.ExitCodeFromError(err); got != exitcodes.ExitNoInput {
			t.Errorf("ExitCodeFromError(err) = %d, want %d", got, exitcodes.ExitNoInput)
		}
	})

	t.Run("invalid TOML", func(t *testing.T) {
		path := filepath.Join(dir, "broken.toml")
		if err := os.WriteFile(path, []byte("[profile\nname ="), 0o644); err != nil {
			t.Fatalf("WriteFile: %v", err)
		}
		if _, err := LoadFile(path); err == nil {
			t.Fatal("LoadFile() error = nil, want error for invalid TOML")
		}
	})

	t.Run("missing name", func(t *testing.T) {
		path := filepath.Join(dir, "noname.toml")
		if err := os.WriteFile(path, []byte("[servers.vault]\nenabled = true\n"), 0o644); err != nil {
			t.Fatalf("WriteFile: %v", err)
		}
		_, err := LoadFile(path)
		if err == nil {
			t.Fatal("LoadFile() error = nil, want error for a missing profile name")
		}
		if got := exitcodes.ExitCodeFromError(err); got != exitcodes.ExitNoInput {
			t.Errorf("ExitCodeFromError(err) = %d, want %d", got, exitcodes.ExitNoInput)
		}
	})

	t.Run("unsafe name", func(t *testing.T) {
		path := filepath.Join(dir, "unsafe.toml")
		contents := "[profile]\nname = \"../../evil\"\n"
		if err := os.WriteFile(path, []byte(contents), 0o644); err != nil {
			t.Fatalf("WriteFile: %v", err)
		}
		if _, err := LoadFile(path); err == nil {
			t.Fatal("LoadFile() error = nil, want error for an unsafe profile name")
		}
	})

	t.Run("name mismatch impossible by construction", func(t *testing.T) {
		// LoadFile derives the name from the file itself, so the
		// filename never needs to match — the file may be named anything.
		path := filepath.Join(dir, "whatever-name.toml")
		contents := "[profile]\nname = \"room\"\n"
		if err := os.WriteFile(path, []byte(contents), 0o644); err != nil {
			t.Fatalf("WriteFile: %v", err)
		}
		p, err := LoadFile(path)
		if err != nil {
			t.Fatalf("LoadFile() error = %v, want nil", err)
		}
		if p.Name != "room" {
			t.Errorf("Name = %q, want %q", p.Name, "room")
		}
	})
}
