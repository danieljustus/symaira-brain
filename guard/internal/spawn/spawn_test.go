package spawn

import (
	"testing"

	"github.com/danieljustus/symaira-guard/internal/config"
	"github.com/danieljustus/symaira-guard/internal/discovery"
)

func TestAllowlist_Allows(t *testing.T) {
	entries := []config.SpawnEntry{
		{Path: "/usr/local/bin/node", ArgvPrefix: []string{"server.js"}},
		{Path: "/opt/homebrew/bin/uvx"},
	}

	tests := []struct {
		name   string
		server discovery.Server
		want   bool
	}{
		{
			name:   "exact path and argv prefix match",
			server: discovery.Server{Command: "/usr/local/bin/node", Args: []string{"server.js", "--port", "3000"}, Transport: discovery.TransportStdio},
			want:   true,
		},
		{
			name:   "entry without argv prefix matches any args",
			server: discovery.Server{Command: "/opt/homebrew/bin/uvx", Args: []string{"--isolated", "mcp-server"}, Transport: discovery.TransportStdio},
			want:   true,
		},
		{
			name:   "entry without argv prefix matches no args",
			server: discovery.Server{Command: "/opt/homebrew/bin/uvx", Transport: discovery.TransportStdio},
			want:   true,
		},
		{
			name:   "path match but argv prefix mismatch",
			server: discovery.Server{Command: "/usr/local/bin/node", Args: []string{"other.js"}, Transport: discovery.TransportStdio},
			want:   false,
		},
		{
			name:   "argv prefix longer than server args",
			server: discovery.Server{Command: "/usr/local/bin/node", Args: []string{"server.js"}, Transport: discovery.TransportStdio},
			want:   true,
		},
		{
			name:   "path not on allowlist",
			server: discovery.Server{Command: "/usr/bin/python3", Args: []string{"server.py"}, Transport: discovery.TransportStdio},
			want:   false,
		},
		{
			name:   "relative command never matches",
			server: discovery.Server{Command: "node", Args: []string{"server.js"}, Transport: discovery.TransportStdio},
			want:   false,
		},
		{
			name:   "path with redundant elements is cleaned",
			server: discovery.Server{Command: "/usr/local/bin/./node", Args: []string{"server.js"}, Transport: discovery.TransportStdio},
			want:   true,
		},
		{
			name:   "http server is not gated",
			server: discovery.Server{Command: "https://mcp.example.com/sse", Transport: discovery.TransportHTTP},
			want:   true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			al := NewAllowlist(entries)
			if got := al.Allows(tt.server); got != tt.want {
				t.Errorf("Allows(%+v) = %v, want %v", tt.server, got, tt.want)
			}
		})
	}
}

func TestAllowlist_DenyByDefault(t *testing.T) {
	server := discovery.Server{Command: "/usr/local/bin/node", Args: []string{"server.js"}, Transport: discovery.TransportStdio}

	tests := []struct {
		name    string
		entries []config.SpawnEntry
		want    bool
	}{
		{"empty allowlist denies everything", nil, false},
		{"relative entry path is ignored", []config.SpawnEntry{{Path: "node"}}, false},
		{"matching entry allows", []config.SpawnEntry{{Path: "/usr/local/bin/node"}}, true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			al := NewAllowlist(tt.entries)
			if got := al.Allows(server); got != tt.want {
				t.Errorf("Allows() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestAllowlist_Len(t *testing.T) {
	tests := []struct {
		name    string
		entries []config.SpawnEntry
		want    int
	}{
		{"empty", nil, 0},
		{"one", []config.SpawnEntry{{Path: "/usr/local/bin/node"}}, 1},
		{"two", []config.SpawnEntry{{Path: "/usr/local/bin/node"}, {Path: "/opt/homebrew/bin/uvx"}}, 2},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := NewAllowlist(tt.entries).Len(); got != tt.want {
				t.Errorf("Len() = %d, want %d", got, tt.want)
			}
		})
	}
}
