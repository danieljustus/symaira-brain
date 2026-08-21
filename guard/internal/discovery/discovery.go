// Package discovery finds and parses MCP server configurations from common AI
// client applications. It supports Hermes, Claude Desktop, Cursor, VS Code
// (and compatible clients), and OpenCode.
//
// Each client stores MCP configuration in a JSON file at a known location.
// Discovery reads those files, parses the client-specific format, and
// normalises servers into a common [Server] representation. [ScanAll]
// reports every source that cannot be mapped as a [Finding]; the legacy
// [DiscoverAll] and [ParseClient] helpers still skip missing files silently.
package discovery

import (
	"encoding/json"
	"fmt"
	"maps"
	"os"
	"path/filepath"
	"runtime"
	"strings"
)

// Transport describes how a server is reached.
type Transport string

const (
	TransportStdio Transport = "stdio"
	TransportHTTP  Transport = "http"
)

// Client identifies a supported AI client application.
type Client string

const (
	ClientHermes        Client = "hermes"
	ClientClaudeDesktop Client = "claude-desktop"
	ClientCursor        Client = "cursor"
	ClientVSCode        Client = "vscode"
	ClientOpenCode      Client = "opencode"
)

// Server is the normalised representation of a single MCP server entry
// discovered from any supported client. EnvValues holds the original values;
// callers that display or log Server should replace them with redacted
// placeholders (e.g. "REDACTED") rather than printing raw secrets.
type Server struct {
	// Name is the server's key in the client's mcpServers map.
	Name string

	// Client is the originating AI client.
	Client Client

	// Command is the executable or URL to invoke/connect.
	Command string

	// Args are extra arguments for stdio servers.
	Args []string

	// EnvKeys holds environment variable names. EnvValues holds the
	// corresponding values (may contain secrets — do not display).
	EnvKeys   []string
	EnvValues []string

	// Transport is the connection type (stdio or http).
	Transport Transport
}

// FS abstracts filesystem access so discovery can be tested without real files.
type FS interface {
	ReadFile(name string) ([]byte, error)
}

// osFS is the default [FS] backed by [os.ReadFile].
type osFS struct{}

func (osFS) ReadFile(name string) ([]byte, error) { return os.ReadFile(name) }

// homeDir returns the user's home directory using a best-effort approach.
func homeDir() string {
	if h, err := os.UserHomeDir(); err == nil {
		return h
	}
	if h := os.Getenv("HOME"); h != "" {
		return h
	}
	return "."
}

// DiscoverAll scans all supported clients for MCP server configurations and
// returns the combined, normalised list. Files that do not exist are skipped.
func DiscoverAll() ([]Server, error) {
	return DiscoverAllWithFS(osFS{})
}

// DiscoverAllWithFS is like [DiscoverAll] but uses the provided [FS] for file
// access, making it straightforward to test.
func DiscoverAllWithFS(fsys FS) ([]Server, error) {
	var all []Server
	for _, src := range clientSources() {
		servers, err := ParseClientWithFS(fsys, src.Client, src.Path)
		if err != nil {
			return nil, err
		}
		all = append(all, servers...)
	}
	return all, nil
}

// clientSource pairs a client identifier with its config file path.
type clientSource struct {
	Client Client
	Path   string
}

// clientSources returns the config file locations for all supported clients
// on the current platform.
func clientSources() []clientSource {
	return clientSourcesForGOOS(runtime.GOOS, homeDir())
}

// clientSourcesForGOOS is the testable inner implementation of clientSources.
// It accepts the OS name and home directory as parameters so that both the
// macOS and Linux branches can be exercised on any platform.
func clientSourcesForGOOS(goos, home string) []clientSource {
	sources := []clientSource{
		// Hermes — cross-platform.
		{ClientHermes, filepath.Join(home, ".config", "hermes", "config.json")},

		// Cursor — cross-platform.
		{ClientCursor, filepath.Join(home, ".cursor", "mcp.json")},

		// VS Code (and Cline/Roo/Continue) — cross-platform global/user scope.
		{ClientVSCode, filepath.Join(home, ".vscode", "mcp.json")},

		// OpenCode — cross-platform.
		{ClientOpenCode, filepath.Join(home, ".config", "opencode", "config.json")},
	}

	// Claude Desktop — platform-specific path.
	if goos == "darwin" {
		sources = append(sources, clientSource{
			ClientClaudeDesktop,
			filepath.Join(home, "Library", "Application Support", "Claude", "claude_desktop_config.json"),
		})
	} else {
		// Linux / other: XDG-style or ~/.config/claude/claude_desktop_config.json
		xdg := os.Getenv("XDG_CONFIG_HOME")
		if xdg == "" {
			xdg = filepath.Join(home, ".config")
		}
		sources = append(sources, clientSource{
			ClientClaudeDesktop,
			filepath.Join(xdg, "claude", "claude_desktop_config.json"),
		})
	}

	return sources
}

// ParseClient reads and parses the MCP config for a single client at the
// given path. If the file does not exist, an empty slice is returned with no
// error.
func ParseClient(client Client, path string) ([]Server, error) {
	return ParseClientWithFS(osFS{}, client, path)
}

// ParseClientWithFS is like [ParseClient] but uses the provided [FS].
// Findings produced while parsing are discarded; use [ScanAllWithFS] when
// every unmappable source must be reported.
func ParseClientWithFS(fsys FS, client Client, path string) ([]Server, error) {
	data, err := fsys.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, fmt.Errorf("discovery: read %s config %s: %w", client, path, err)
	}

	servers, _, err := parseClientData(client, path, data)
	if err != nil {
		return nil, fmt.Errorf("discovery: parse %s config: %w", client, err)
	}
	return servers, nil
}

// ---------------------------------------------------------------------------
// Standard mcpServers format (Hermes, Claude Desktop, Cursor, VS Code)
// ---------------------------------------------------------------------------

// rawConfig represents the top-level JSON structure used by most clients.
// The actual MCP servers live under the "mcpServers" key.
type rawConfig struct {
	MCPServers map[string]rawServer `json:"mcpServers"`
}

// rawServer is a single server entry within an mcpServers map.
type rawServer struct {
	Command string            `json:"command"`
	Args    []string          `json:"args"`
	URL     string            `json:"url"`
	Env     map[string]string `json:"env"`
}

func parseMCPserversFormat(client Client, path string, data []byte) ([]Server, []Finding, error) {
	var cfg rawConfig
	if err := json.Unmarshal(data, &cfg); err != nil {
		return nil, nil, err
	}
	servers, findings := rawServersToServers(client, path, cfg.MCPServers)
	return servers, findings, nil
}

// rawServersToServers normalises a raw mcpServers map into Server values.
// Entries that cannot be mapped (neither command nor url) become findings.
func rawServersToServers(client Client, path string, raw map[string]rawServer) ([]Server, []Finding) {
	if len(raw) == 0 {
		return nil, nil
	}
	servers := make([]Server, 0, len(raw))
	var findings []Finding
	for name, rs := range raw {
		s := Server{
			Name:    name,
			Client:  client,
			Command: rs.Command,
			Args:    rs.Args,
		}

		// Determine transport: URL → HTTP, otherwise stdio.
		if rs.URL != "" {
			s.Transport = TransportHTTP
			if s.Command == "" {
				s.Command = rs.URL
			}
		} else {
			s.Transport = TransportStdio
		}

		// An entry with neither a command nor a URL cannot be launched or
		// connected — report it instead of silently skipping it.
		if s.Command == "" {
			findings = append(findings, Finding{
				Client:  client,
				Path:    path,
				Status:  StatusUnsupported,
				Message: fmt.Sprintf("server %q is missing both command and url", name),
			})
			continue
		}

		// Env vars — preserve keys, redact values.
		if len(rs.Env) > 0 {
			s.EnvKeys = make([]string, 0, len(rs.Env))
			s.EnvValues = make([]string, 0, len(rs.Env))
			for k, v := range rs.Env {
				s.EnvKeys = append(s.EnvKeys, k)
				s.EnvValues = append(s.EnvValues, v)
			}
		}

		servers = append(servers, s)
	}
	return servers, findings
}

// ---------------------------------------------------------------------------
// OpenCode format — uses "mcp" key with type-based entries
// ---------------------------------------------------------------------------

// openCodeConfig is the top-level structure for ~/.config/opencode/config.json.
type openCodeConfig struct {
	MCP map[string]openCodeServer `json:"mcp"`
}

// openCodeServer represents a single MCP server entry in OpenCode's format.
type openCodeServer struct {
	Type        string            `json:"type"`        // "local" or "remote"
	Command     string            `json:"command"`     // for local
	Args        []string          `json:"args"`        // for local
	URL         string            `json:"url"`         // for remote
	Environment map[string]string `json:"environment"` // note: "environment", not "env"
	Env         map[string]string `json:"env"`         // some versions may use "env"
}

func parseOpenCodeFormat(client Client, path string, data []byte) ([]Server, []Finding, error) {
	var cfg openCodeConfig
	if err := json.Unmarshal(data, &cfg); err != nil {
		return nil, nil, err
	}
	if len(cfg.MCP) == 0 {
		return nil, nil, nil
	}

	servers := make([]Server, 0, len(cfg.MCP))
	var findings []Finding
	for name, oc := range cfg.MCP {
		s := Server{
			Name:   name,
			Client: client,
		}

		// Resolve transport type.
		switch strings.ToLower(oc.Type) {
		case "remote":
			s.Transport = TransportHTTP
			s.Command = oc.URL
			if s.Command == "" {
				findings = append(findings, Finding{
					Client:  client,
					Path:    path,
					Status:  StatusUnsupported,
					Message: fmt.Sprintf("server %q is remote but has no url", name),
				})
				continue
			}
		case "local", "":
			// "local" or unspecified — default to stdio.
			s.Transport = TransportStdio
			s.Command = oc.Command
			s.Args = oc.Args
			if s.Command == "" {
				findings = append(findings, Finding{
					Client:  client,
					Path:    path,
					Status:  StatusUnsupported,
					Message: fmt.Sprintf("server %q is local but has no command", name),
				})
				continue
			}
		default:
			// Unknown type — try stdio, but flag the guess as approximate.
			s.Transport = TransportStdio
			s.Command = oc.Command
			s.Args = oc.Args
			if s.Command == "" {
				findings = append(findings, Finding{
					Client:  client,
					Path:    path,
					Status:  StatusUnsupported,
					Message: fmt.Sprintf("server %q has unknown type %q and no command", name, oc.Type),
				})
				continue
			}
			findings = append(findings, Finding{
				Client:  client,
				Path:    path,
				Status:  StatusApproximate,
				Message: fmt.Sprintf("server %q has unknown type %q, treated as local", name, oc.Type),
			})
		}

		// Merge env from both "environment" and "env" keys (some versions differ).
		merged := mergeEnvMaps(oc.Environment, oc.Env)
		if len(merged) > 0 {
			s.EnvKeys = make([]string, 0, len(merged))
			s.EnvValues = make([]string, 0, len(merged))
			for k, v := range merged {
				s.EnvKeys = append(s.EnvKeys, k)
				s.EnvValues = append(s.EnvValues, v)
			}
		}

		servers = append(servers, s)
	}
	return servers, findings, nil
}

// mergeEnvMaps combines two env maps. If both contain the same key, primary wins.
func mergeEnvMaps(primary, secondary map[string]string) map[string]string {
	if len(primary) == 0 && len(secondary) == 0 {
		return nil
	}
	out := make(map[string]string, len(primary)+len(secondary))
	maps.Copy(out, secondary)
	maps.Copy(out, primary)
	return out
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// RedactedEnv returns a copy of EnvValues where every value is replaced with
// "REDACTED". Useful for logging or display.
func (s Server) RedactedEnv() map[string]string {
	out := make(map[string]string, len(s.EnvKeys))
	for _, k := range s.EnvKeys {
		out[k] = "REDACTED"
	}
	return out
}

// String returns a short human-readable description of the server.
func (s Server) String() string {
	var b strings.Builder
	fmt.Fprintf(&b, "%s (%s/%s)", s.Name, s.Client, s.Transport)
	if s.Command != "" {
		fmt.Fprintf(&b, " → %s", s.Command)
	}
	if len(s.Args) > 0 {
		fmt.Fprintf(&b, " %s", strings.Join(s.Args, " "))
	}
	return b.String()
}
