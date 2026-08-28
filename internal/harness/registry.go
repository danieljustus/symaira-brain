package harness

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-corekit/exitcodes"
)

// Name identifies one harness known to symbrain.
type Name string

// The harness names known to symbrain.  Capability-only entries are
// included here too, so downstream registries cannot silently drift.
const (
	Claude        Name = "claude"
	ClaudeDesktop Name = "claude-desktop"
	Cursor        Name = "cursor"
	Opencode      Name = "opencode"
	Codex         Name = "codex"
	Gemini        Name = "gemini"
	Agents        Name = "agents"
	Hermes        Name = "hermes"
	Antigravity   Name = "antigravity"
	OpenClaw      Name = "openclaw"
)

// InstructionAdapter identifies the instruction-file adapter used by a
// harness.  The value is an adapter kind, not another harness registry.
type InstructionAdapter string

const (
	InstructionAdapterNone   InstructionAdapter = ""
	InstructionAdapterAgents InstructionAdapter = "agents"
	InstructionAdapterClaude InstructionAdapter = "claude"
	InstructionAdapterCursor InstructionAdapter = "cursor"
	InstructionAdapterGemini InstructionAdapter = "gemini"
)

// SkillTarget identifies the in-process skills renderer target for a harness.
type SkillTarget string

const (
	SkillTargetNone        SkillTarget = ""
	SkillTargetOpenCode    SkillTarget = "opencode"
	SkillTargetClaude      SkillTarget = "claude"
	SkillTargetCodex       SkillTarget = "codex"
	SkillTargetHermes      SkillTarget = "hermes"
	SkillTargetAntigravity SkillTarget = "antigravity"
	SkillTargetOpenClaw    SkillTarget = "openclaw"
)

// Format is the on-disk serialization of a harness's MCP config file.
type Format string

const (
	// FormatJSON configs are edited structurally via encoding/json.
	FormatJSON Format = "json"
	// FormatTOML configs are edited structurally via github.com/BurntSushi/toml.
	FormatTOML Format = "toml"
)

// ServerName is the key symbrain registers its MCP entry under in every
// harness's server map (e.g. mcpServers.symbrain, [mcp_servers.symbrain]).
const ServerName = "symbrain"

// Harness describes one known AI harness: its optional MCP config, and
// the instruction and skills capabilities symbrain can use with it.
type Harness struct {
	// Name is the stable identifier used on the CLI (--harness <name>).
	Name Name
	// DisplayName is the human-readable name shown by inspection commands.
	DisplayName string
	// Format is the config file's serialization.
	Format Format
	// ServersKey is the top-level key/table that holds the map of
	// server-name -> entry (e.g. "mcpServers" for the JSON harnesses,
	// "mcp_servers" for codex's TOML).
	ServersKey string

	// ConfigPath resolves this harness's primary (global, user-level)
	// config file path.
	ConfigPath func() (string, error)

	// SupportsMCPInstall reports whether symbrain can install its MCP
	// server entry into this harness's configuration.
	SupportsMCPInstall bool
	// InstructionAdapter selects the instruction-file adapter, when one
	// exists. An empty value is an intentional unsupported capability.
	InstructionAdapter InstructionAdapter
	// SkillTarget selects the in-process skills renderer target, when one
	// exists. An empty value is an intentional unsupported capability.
	SkillTarget SkillTarget

	// SupportsProject reports whether this harness also has a
	// project-local config file in addition to its global one.
	SupportsProject bool
	// ProjectConfigPath resolves the project-local config path rooted at
	// dir. Only meaningful when SupportsProject is true.
	ProjectConfigPath func(dir string) string
}

// All is the single registry of every harness known to symbrain. Entries
// that do not support MCP installation still participate in instruction or
// skills derivation when they declare those capabilities.
var All = []Harness{
	{
		Name:               Claude,
		DisplayName:        "Claude Code",
		Format:             FormatJSON,
		SupportsMCPInstall: true,
		InstructionAdapter: InstructionAdapterClaude,
		SkillTarget:        SkillTargetClaude,
		ServersKey:         "mcpServers",
		ConfigPath:         homeJoin(".claude.json"),
		SupportsProject:    true,
		ProjectConfigPath: func(dir string) string {
			return filepath.Join(dir, ".mcp.json")
		},
	},
	{
		Name:               ClaudeDesktop,
		DisplayName:        "Claude Desktop",
		Format:             FormatJSON,
		SupportsMCPInstall: true,
		InstructionAdapter: InstructionAdapterNone,
		SkillTarget:        SkillTargetNone,
		ServersKey:         "mcpServers",
		ConfigPath:         claudeDesktopConfigPath,
	},
	{
		Name:               Cursor,
		DisplayName:        "Cursor",
		Format:             FormatJSON,
		SupportsMCPInstall: true,
		InstructionAdapter: InstructionAdapterCursor,
		SkillTarget:        SkillTargetNone,
		ServersKey:         "mcpServers",
		ConfigPath:         homeJoin(".cursor", "mcp.json"),
	},
	{
		Name:               Opencode,
		DisplayName:        "OpenCode",
		Format:             FormatJSON,
		SupportsMCPInstall: true,
		InstructionAdapter: InstructionAdapterNone,
		SkillTarget:        SkillTargetOpenCode,
		ServersKey:         "mcpServers",
		ConfigPath:         xdgConfigJoin("opencode", "config.json"),
	},
	{
		Name:               Codex,
		DisplayName:        "Codex CLI",
		Format:             FormatTOML,
		SupportsMCPInstall: true,
		InstructionAdapter: InstructionAdapterNone,
		SkillTarget:        SkillTargetCodex,
		ServersKey:         "mcp_servers",
		ConfigPath:         homeJoin(".codex", "config.toml"),
	},
	{
		Name:               Gemini,
		DisplayName:        "Gemini CLI",
		Format:             FormatJSON,
		SupportsMCPInstall: true,
		InstructionAdapter: InstructionAdapterGemini,
		SkillTarget:        SkillTargetNone,
		ServersKey:         "mcpServers",
		ConfigPath:         homeJoin(".gemini", "settings.json"),
	},
	{
		Name:               Agents,
		DisplayName:        "AGENTS.md",
		SupportsMCPInstall: false,
		InstructionAdapter: InstructionAdapterAgents,
		SkillTarget:        SkillTargetNone,
		ConfigPath:         unsupportedConfigPath,
	},
	{
		Name:               Hermes,
		DisplayName:        "Hermes",
		SupportsMCPInstall: false,
		InstructionAdapter: InstructionAdapterNone,
		SkillTarget:        SkillTargetHermes,
		ConfigPath:         unsupportedConfigPath,
	},
	{
		Name:               Antigravity,
		DisplayName:        "Antigravity",
		SupportsMCPInstall: false,
		InstructionAdapter: InstructionAdapterNone,
		SkillTarget:        SkillTargetAntigravity,
		ConfigPath:         unsupportedConfigPath,
	},
	{
		Name:               OpenClaw,
		DisplayName:        "OpenClaw",
		SupportsMCPInstall: false,
		InstructionAdapter: InstructionAdapterNone,
		SkillTarget:        SkillTargetOpenClaw,
		ConfigPath:         unsupportedConfigPath,
	},
}

// Lookup finds a registered harness by name. An unknown name is reported as
// a typed *exitcodes.CLIError (ExitNoInput) so callers can refuse cleanly
// instead of guessing at a config location.
func Lookup(name string) (Harness, error) {
	for _, h := range All {
		if string(h.Name) == name {
			return h, nil
		}
	}
	return Harness{}, exitcodes.Wrap(
		fmt.Errorf("unknown harness %q", name),
		exitcodes.ExitNoInput,
		exitcodes.KindValidation,
		"harness: want one of: "+strings.Join(Names(), ", "),
	)
}

// Names returns every registered harness name, in registry order.
func Names() []string {
	names := make([]string, len(All))
	for i, h := range All {
		names[i] = string(h.Name)
	}
	sort.Strings(names)
	return names
}

func unsupportedConfigPath() (string, error) {
	return "", fmt.Errorf("harness does not support MCP installation")
}

// homeJoin returns a ConfigPath resolver for $HOME/elem...
func homeJoin(elem ...string) func() (string, error) {
	return func() (string, error) {
		home, err := os.UserHomeDir()
		if err != nil {
			return "", fmt.Errorf("resolve home directory: %w", err)
		}
		parts := append([]string{home}, elem...)
		return filepath.Join(parts...), nil
	}
}

// xdgConfigJoin returns a ConfigPath resolver for
// $XDG_CONFIG_HOME/elem... (default $HOME/.config/elem...), matching the
// XDG Base Directory convention used across the Symaira tools.
func xdgConfigJoin(elem ...string) func() (string, error) {
	return func() (string, error) {
		if base := os.Getenv("XDG_CONFIG_HOME"); base != "" {
			parts := append([]string{base}, elem...)
			return filepath.Join(parts...), nil
		}
		home, err := os.UserHomeDir()
		if err != nil {
			return "", fmt.Errorf("resolve home directory: %w", err)
		}
		parts := append([]string{home, ".config"}, elem...)
		return filepath.Join(parts...), nil
	}
}

// claudeDesktopConfigPath resolves Claude Desktop's config file, which
// lives in a different location per OS:
//
//   - macOS:   ~/Library/Application Support/Claude/claude_desktop_config.json
//   - Linux:   $XDG_CONFIG_HOME/Claude/claude_desktop_config.json
//     (default ~/.config/Claude/claude_desktop_config.json)
//   - Windows: %APPDATA%\Claude\claude_desktop_config.json
func claudeDesktopConfigPath() (string, error) {
	return resolveClaudeDesktopConfigPath(runtime.GOOS, os.Getenv)
}

// resolveClaudeDesktopConfigPath is the testable core of
// claudeDesktopConfigPath: goos and getenv are injected so tests can cover
// every platform branch regardless of the OS actually running the test.
func resolveClaudeDesktopConfigPath(goos string, getenv func(string) string) (string, error) {
	const filename = "claude_desktop_config.json"

	switch goos {
	case "darwin":
		home, err := userHomeDirFor(getenv)
		if err != nil {
			return "", err
		}
		return filepath.Join(home, "Library", "Application Support", "Claude", filename), nil
	case "windows":
		if appData := getenv("APPDATA"); appData != "" {
			return filepath.Join(appData, "Claude", filename), nil
		}
		home, err := userHomeDirFor(getenv)
		if err != nil {
			return "", err
		}
		return filepath.Join(home, "AppData", "Roaming", "Claude", filename), nil
	default: // linux and other XDG-style platforms
		if base := getenv("XDG_CONFIG_HOME"); base != "" {
			return filepath.Join(base, "Claude", filename), nil
		}
		home, err := userHomeDirFor(getenv)
		if err != nil {
			return "", err
		}
		return filepath.Join(home, ".config", "Claude", filename), nil
	}
}

// userHomeDirFor mirrors os.UserHomeDir's platform lookup but reads through
// the injected getenv, so resolveClaudeDesktopConfigPath is fully
// deterministic under test.
func userHomeDirFor(getenv func(string) string) (string, error) {
	env := "HOME"
	if runtime.GOOS == "windows" {
		env = "USERPROFILE"
	}
	if v := getenv(env); v != "" {
		return v, nil
	}
	// Fall back to the real resolver for the environment this test double
	// doesn't cover (e.g. darwin/linux tests reading $HOME, which getenv
	// already handles above in practice).
	return os.UserHomeDir()
}
