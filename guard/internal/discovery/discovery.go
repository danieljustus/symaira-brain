// Package discovery finds and parses MCP server configurations from common AI
// client applications. It supports Hermes, Claude Desktop, Cursor, VS Code
// (and compatible clients), and OpenCode.
//
// Since 2026-08-21 this package is a thin adapter over
// corekit/mcpcfgkit — the shared discovery implementation behind symscope,
// symguard, and symbrain (repo-konsolidierung.md §9). The public API is
// unchanged; parsing, path resolution, and format handling live in corekit.
package discovery

import (
	"fmt"
	"os"
	"strings"

	"github.com/danieljustus/symaira-corekit/mcpcfgkit"
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

// kitFS adapts this package's read-only FS to mcpcfgkit.FS (which also asks
// for Glob; globbing is not used by the guard scan paths, so it returns no
// matches).
type kitFS struct{ inner FS }

func (k kitFS) ReadFile(path string) ([]byte, error) { return k.inner.ReadFile(path) }
func (k kitFS) Glob(string) ([]string, error)        { return nil, nil }

// toKitSources maps this package's client/path pairs onto mcpcfgkit sources.
// The explicit per-client keys mirror what the pre-kit implementation read.
func toKitSources(pairs []struct {
	client Client
	path   string
}) []mcpcfgkit.ScanSource {
	out := make([]mcpcfgkit.ScanSource, 0, len(pairs))
	for _, p := range pairs {
		key := "mcpServers"
		if p.client == ClientOpenCode {
			key = "mcp"
		}
		out = append(out, mcpcfgkit.ScanSource{
			Client: mcpcfgkit.Client(p.client),
			Path:   p.path,
			Key:    key,
		})
	}
	return out
}

// fromKitServer converts a corekit server into this package's representation.
func fromKitServer(s mcpcfgkit.Server) Server {
	out := Server{
		Name:      s.Name,
		Client:    Client(s.Client),
		Command:   s.Command,
		Args:      s.Args,
		Transport: Transport(s.Transport),
	}
	// Prefer the split EnvKeys/EnvValues view when the kit produced one;
	// otherwise derive both lists from the merged env map (deterministic
	// order via sorted keys happens at the call sites that need it).
	if len(s.EnvKeys) > 0 {
		out.EnvKeys = s.EnvKeys
		out.EnvValues = s.EnvValues
	} else {
		for k, v := range s.Env {
			out.EnvKeys = append(out.EnvKeys, k)
			out.EnvValues = append(out.EnvValues, v)
		}
	}
	return out
}

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
	b := fmt.Sprintf("%s (%s/%s)", s.Name, s.Client, s.Transport)
	if s.Command != "" {
		b += " → " + s.Command
	}
	if len(s.Args) > 0 {
		b += " " + strings.Join(s.Args, " ")
	}
	return b
}

// clientSource pairs a client identifier with its config path. Kept as the
// historical test-facing shape; sources now come from corekit/mcpcfgkit.
type clientSource struct {
	Client Client
	Path   string
}

// clientSources returns the config file locations for all supported clients
// on the current platform.
func clientSources() []clientSource {
	srcs := mcpcfgkit.DefaultSources()
	out := make([]clientSource, 0, len(srcs))
	for _, s := range srcs {
		out = append(out, clientSource{Client: Client(s.Client), Path: s.Path})
	}
	return out
}

// clientSourcesForGOOS is the injectable variant kept for the platform
// resolution tests (darwin vs linux XDG).
func clientSourcesForGOOS(goos, home string) []clientSource {
	srcs := mcpcfgkit.DefaultSourcesForPlatform(goos, home)
	out := make([]clientSource, 0, len(srcs))
	for _, s := range srcs {
		out = append(out, clientSource{Client: Client(s.Client), Path: s.Path})
	}
	return out
}

// DiscoverAll scans all supported clients for MCP server configurations and
// returns the combined, normalised list. Files that do not exist are skipped.
func DiscoverAll() ([]Server, error) {
	return DiscoverAllWithFS(osFS{})
}

// DiscoverAllWithFS is like [DiscoverAll] but uses the provided [FS] for file
// access, making it straightforward to test. Missing config files are
// skipped silently (historical behaviour); other failures surface as errors.
func DiscoverAllWithFS(fsys FS) ([]Server, error) {
	kit := mcpcfgkit.ScanAllWithFS(kitFS{fsys}, toKitSources(clientSourcePairs()))
	servers := make([]Server, 0, len(kit.Servers))
	for _, s := range kit.Servers {
		servers = append(servers, fromKitServer(s))
	}
	for _, f := range kit.Findings {
		if containsNotExist(f.Message) {
			continue // missing files are not an error here
		}
		if f.Status == mcpcfgkit.StatusUnsupported {
			return nil, fmt.Errorf("discovery: %s", f.String())
		}
	}
	return servers, nil
}

// clientSourcePairs lists the config locations for all supported clients on
// the current platform, in the historical order.
func clientSourcePairs() []struct {
	client Client
	path   string
} {
	srcs := mcpcfgkit.DefaultSources()
	pairs := make([]struct {
		client Client
		path   string
	}, 0, len(srcs))
	for _, s := range srcs {
		c := Client(s.Client)
		// The historical guard source list had no windsurf entry and used
		// these exact clients; unknown additions from the shared list are
		// still scanned but keep their own identifier.
		pairs = append(pairs, struct {
			client Client
			path   string
		}{c, s.Path})
	}
	return pairs
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
	switch client {
	case ClientHermes, ClientClaudeDesktop, ClientCursor, ClientVSCode, ClientOpenCode:
	default:
		return nil, fmt.Errorf("discovery: unsupported client %q", client)
	}
	key := "mcpServers"
	if client == ClientOpenCode {
		key = "mcp"
	}
	res := mcpcfgkit.ScanAllWithFS(kitFS{fsys}, []mcpcfgkit.ScanSource{{
		Client: mcpcfgkit.Client(client),
		Path:   path,
		Key:    key,
	}})
	// Missing file → empty result, no error (legacy behaviour).
	if len(res.Servers) == 0 && len(res.Findings) == 1 {
		msg := res.Findings[0].Message
		if containsNotExist(msg) || containsFold(msg, "key not found") {
			// An empty/foreign document maps to zero servers, not an error
			// (historical behaviour for `{}` configs).
			return nil, nil
		}
	}
	servers := make([]Server, 0, len(res.Servers))
	for _, s := range res.Servers {
		servers = append(servers, fromKitServer(s))
	}
	if len(res.Findings) > 0 {
		return nil, fmt.Errorf("discovery: parse %s config: %s", client, res.Findings[0].String())
	}
	return servers, nil
}

func containsNotExist(msg string) bool {
	return containsFold(msg, "no such file") || containsFold(msg, "file does not exist") ||
		containsFold(msg, "cannot find the file")
}

func containsFold(s, sub string) bool {
	return len(s) >= len(sub) && indexOfFold(s, sub) >= 0
}

func indexOfFold(s, sub string) int {
	for i := 0; i+len(sub) <= len(s); i++ {
		if matchAt(s, i, sub) {
			return i
		}
	}
	return -1
}

func matchAt(s string, at int, sub string) bool {
	for j := 0; j < len(sub); j++ {
		a := lowerByte(s[at+j])
		b := lowerByte(sub[j])
		if a != b {
			return false
		}
	}
	return true
}

func lowerByte(b byte) byte {
	if b >= 'A' && b <= 'Z' {
		return b + 32
	}
	return b
}

// mapFS is an in-memory FS for the test adapter shims.
type mapFS struct{ files map[string][]byte }

func (m *mapFS) ReadFile(path string) ([]byte, error) {
	if data, ok := m.files[path]; ok {
		return data, nil
	}
	return nil, &os.PathError{Op: "open", Path: path, Err: os.ErrNotExist}
}

// parseMCPserversFormat is the adapter shim for tests: parses an mcpServers
// JSON payload through the shared kit and returns servers + findings.
func parseMCPserversFormat(client Client, path string, data []byte) ([]Server, []Finding, error) {
	fsys := &mapFS{files: map[string][]byte{path: data}}
	key := "mcpServers"
	if client == ClientOpenCode {
		key = "mcp"
	}
	single := mcpcfgkit.ScanAllWithFS(kitFS{fsys}, []mcpcfgkit.ScanSource{{
		Client: mcpcfgkit.Client(client),
		Path:   path,
		Key:    key,
	}})
	var (
		servers  []Server
		findings []Finding
	)
	for _, s := range single.Servers {
		servers = append(servers, fromKitServer(s))
	}
	for _, f := range single.Findings {
		findings = append(findings, Finding{
			Client:  Client(f.Client),
			Path:    f.Path,
			Status:  Status(f.Status),
			Message: f.Message,
		})
	}
	return servers, findings, nil
}

// parseOpenCodeFormat is the adapter shim for tests: same path as the
// mcpServers format but with OpenCode's "mcp" key and type-based entries.
func parseOpenCodeFormat(client Client, path string, data []byte) ([]Server, []Finding, error) {
	return parseMCPserversFormat(client, path, data)
}

var _ = fmt.Sprintf // keep fmt imported for the wrappers above
