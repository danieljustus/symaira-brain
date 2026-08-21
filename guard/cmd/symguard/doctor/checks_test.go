package doctor

import (
	"bytes"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-guard/internal/config"
	"github.com/danieljustus/symaira-guard/internal/discovery"
	"github.com/danieljustus/symaira-guard/internal/spawn"
)

func TestCheckServers(t *testing.T) {
	al := spawn.NewAllowlist([]config.SpawnEntry{
		{Path: "/usr/local/bin/node", ArgvPrefix: []string{"server.js"}},
	})

	servers := []discovery.Server{
		{
			Name:      "api-tool",
			Client:    discovery.ClientClaudeDesktop,
			Command:   "python",
			Args:      []string{"server.py"},
			Transport: discovery.TransportStdio,
			EnvKeys:   []string{"API_KEY"},
			EnvValues: []string{"sk-ant-abc"},
		},
		{
			Name:      "filesystem",
			Client:    discovery.ClientClaudeDesktop,
			Command:   "/usr/local/bin/node",
			Args:      []string{"server.js", "/tmp"},
			Transport: discovery.TransportStdio,
		},
		{
			Name:      "remote-api",
			Client:    discovery.ClientClaudeDesktop,
			Command:   "https://mcp.example.com/sse",
			Transport: discovery.TransportHTTP,
		},
		{
			Name:      "uvx-tool",
			Client:    discovery.ClientCursor,
			Command:   "/opt/homebrew/bin/uvx",
			Transport: discovery.TransportStdio,
		},
	}

	checks := checkServers(servers, al)
	if len(checks) != 4 {
		t.Fatalf("got %d checks, want 4", len(checks))
	}

	// Results are sorted by client, then name.
	if checks[0].Name != "api-tool" {
		t.Errorf("checks[0].Name = %q, want %q (sorted)", checks[0].Name, "api-tool")
	}

	byName := map[string]ServerCheck{}
	for _, c := range checks {
		byName[c.Name] = c
	}

	if got := byName["api-tool"]; got.Allowed {
		t.Error("api-tool (relative command) should be denied")
	} else if len(got.Secrets) != 1 || got.Secrets[0] != "API_KEY" {
		t.Errorf("api-tool Secrets = %v, want [API_KEY]", got.Secrets)
	}

	if got := byName["filesystem"]; !got.Allowed {
		t.Error("filesystem (allowlisted path + argv prefix) should be allowed")
	}

	if got := byName["remote-api"]; !got.Allowed {
		t.Error("remote-api (http) should not be gated")
	}

	if got := byName["uvx-tool"]; got.Allowed {
		t.Error("uvx-tool (not on allowlist) should be denied")
	}
}

func TestCheckServers_DenyByDefault(t *testing.T) {
	servers := []discovery.Server{
		{Name: "tool", Client: discovery.ClientHermes, Command: "/usr/local/bin/node", Transport: discovery.TransportStdio},
	}
	checks := checkServers(servers, spawn.NewAllowlist(nil))
	if len(checks) != 1 {
		t.Fatalf("got %d checks, want 1", len(checks))
	}
	if checks[0].Allowed {
		t.Error("empty allowlist should deny every stdio server")
	}
}

func TestIssueCount(t *testing.T) {
	tests := []struct {
		name   string
		checks []ServerCheck
		want   int
	}{
		{
			name:   "no issues",
			checks: []ServerCheck{{Name: "a", Allowed: true}, {Name: "b", Allowed: true, Transport: discovery.TransportHTTP}},
			want:   0,
		},
		{
			name:   "denied server",
			checks: []ServerCheck{{Name: "a", Allowed: false}},
			want:   1,
		},
		{
			name:   "plaintext secrets",
			checks: []ServerCheck{{Name: "a", Allowed: true, Secrets: []string{"API_KEY"}}},
			want:   1,
		},
		{
			name:   "denied and secrets count once",
			checks: []ServerCheck{{Name: "a", Allowed: false, Secrets: []string{"API_KEY"}}},
			want:   1,
		},
		{
			name:   "empty",
			checks: nil,
			want:   0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := issueCount(tt.checks); got != tt.want {
				t.Errorf("issueCount() = %d, want %d", got, tt.want)
			}
		})
	}
}

func TestPrintServerChecks(t *testing.T) {
	checks := []ServerCheck{
		{Name: "api-tool", Client: discovery.ClientClaudeDesktop, Command: "python", Args: []string{"server.py"}, Transport: discovery.TransportStdio, Allowed: false},
		{Name: "filesystem", Client: discovery.ClientClaudeDesktop, Command: "/usr/local/bin/node", Args: []string{"server.js"}, Transport: discovery.TransportStdio, Allowed: true},
		{Name: "remote-api", Client: discovery.ClientClaudeDesktop, Command: "https://mcp.example.com/sse", Transport: discovery.TransportHTTP, Allowed: true},
	}

	var buf bytes.Buffer
	printServerChecks(&buf, checks)
	out := buf.String()

	for _, want := range []string{
		"Discovered MCP servers (spawn allowlist):",
		"[DENIED]  api-tool (claude-desktop/stdio) → python server.py (not on spawn allowlist)",
		"[allowed] filesystem (claude-desktop/stdio) → /usr/local/bin/node server.js",
		"[n/a]     remote-api (claude-desktop/http) → https://mcp.example.com/sse",
	} {
		if !strings.Contains(out, want) {
			t.Errorf("output missing %q:\n%s", want, out)
		}
	}
}

func TestPrintServerChecks_Empty(t *testing.T) {
	var buf bytes.Buffer
	printServerChecks(&buf, nil)
	if buf.Len() != 0 {
		t.Errorf("expected no output for empty checks, got %q", buf.String())
	}
}

func TestPrintSecretRisks(t *testing.T) {
	checks := []ServerCheck{
		{Name: "api-tool", Client: discovery.ClientClaudeDesktop, Secrets: []string{"API_KEY", "TOKEN"}},
		{Name: "clean-tool", Client: discovery.ClientHermes},
	}

	var buf bytes.Buffer
	printSecretRisks(&buf, checks)
	out := buf.String()

	for _, want := range []string{
		"Plaintext secret risk:",
		"api-tool (claude-desktop): env API_KEY, TOKEN stored as plaintext values in the client config",
		"symguard reports this risk but is not a secret store — move these values to symvault and reference them at launch time.",
	} {
		if !strings.Contains(out, want) {
			t.Errorf("output missing %q:\n%s", want, out)
		}
	}
	if strings.Contains(out, "clean-tool") {
		t.Errorf("clean-tool must not appear in risk output:\n%s", out)
	}
}

func TestPrintSecretRisks_NoRisks(t *testing.T) {
	var buf bytes.Buffer
	printSecretRisks(&buf, []ServerCheck{{Name: "clean", Allowed: true}})
	if buf.Len() != 0 {
		t.Errorf("expected no output without risks, got %q", buf.String())
	}
}
