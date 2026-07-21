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

// Name identifies one of the harnesses symbrain can install into.
type Name string

// The six harnesses symbrain knows how to configure, per
// docs/ARCHITEKTUR.md §5.9.
const (
	Claude        Name = "claude"
	ClaudeDesktop Name = "claude-desktop"
	Cursor        Name = "cursor"
	Opencode      Name = "opencode"
	Codex         Name = "codex"
	Gemini        Name = "gemini"
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

// Harness describes one supported AI harness: where its MCP config file
// lives, how it is structured, and how symbrain edits it.
type Harness struct {
	// Name is the stable identifier used on the CLI (--harness <name>).
	Name Name
	// Format is the config file's serialization.
	Format Format
	// ServersKey is the top-level key/table that holds the map of
	// server-name -> entry (e.g. "mcpServers" for the JSON harnesses,
	// "mcp_servers" for codex's TOML).
	ServersKey string

	// ConfigPath resolves this harness's primary (global, user-level)
	// config file path.
	ConfigPath func() (string, error)

	// SupportsProject reports whether this harness also has a
	// project-local config file in addition to its global one.
	SupportsProject bool
	// ProjectConfigPath resolves the project-local config path rooted at
	// dir. Only meaningful when SupportsProject is true.
	ProjectConfigPath func(dir string) string
}

// All is the registry of every harness symbrain can install into, in the
// order documented in docs/ARCHITEKTUR.md §5.9.
var All = []Harness{
	{
		Name:            Claude,
		Format:          FormatJSON,
		ServersKey:      "mcpServers",
		ConfigPath:      homeJoin(".claude.json"),
		SupportsProject: true,
		ProjectConfigPath: func(dir string) string {
			return filepath.Join(dir, ".mcp.json")
		},
	},
	{
		Name:       ClaudeDesktop,
		Format:     FormatJSON,
		ServersKey: "mcpServers",
		ConfigPath: claudeDesktopConfigPath,
	},
	{
		Name:       Cursor,
		Format:     FormatJSON,
		ServersKey: "mcpServers",
		ConfigPath: homeJoin(".cursor", "mcp.json"),
	},
	{
		Name:       Opencode,
		Format:     FormatJSON,
		ServersKey: "mcpServers",
		ConfigPath: xdgConfigJoin("opencode", "config.json"),
	},
	{
		Name:       Codex,
		Format:     FormatTOML,
		ServersKey: "mcp_servers",
		ConfigPath: homeJoin(".codex", "config.toml"),
	},
	{
		Name:       Gemini,
		Format:     FormatJSON,
		ServersKey: "mcpServers",
		ConfigPath: homeJoin(".gemini", "settings.json"),
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
