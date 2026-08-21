package scan

import (
	"bytes"
	"encoding/json"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-guard/internal/discovery"
)

// resultFixture returns a Result with two servers (one carrying env values
// that must never be printed) and one finding.
func resultFixture() discovery.Result {
	return discovery.Result{
		Servers: []discovery.Server{
			{
				Name:      "alpha",
				Client:    discovery.ClientHermes,
				Command:   "node",
				Args:      []string{"server.js"},
				Transport: discovery.TransportStdio,
				EnvKeys:   []string{"TOKEN"},
				EnvValues: []string{"supersecret-value"},
			},
			{
				Name:      "beta",
				Client:    discovery.ClientOpenCode,
				Command:   "https://mcp.example.com/sse",
				Transport: discovery.TransportHTTP,
			},
		},
		Findings: []discovery.Finding{
			{
				Client:  discovery.ClientCursor,
				Path:    "/home/test/.cursor/mcp.json",
				Status:  discovery.StatusUnsupported,
				Message: "config file not found",
			},
		},
	}
}

// withFixture replaces the discovery entry point with a fixed Result.
func withFixture(t *testing.T) {
	t.Helper()
	orig := discoverAll
	discoverAll = func() discovery.Result { return resultFixture() }
	t.Cleanup(func() { discoverAll = orig })
}

func TestRun_Table(t *testing.T) {
	withFixture(t)
	var out, errOut bytes.Buffer
	code := Run([]string{"--format", "table"}, &out, &errOut)
	if code != 0 {
		t.Fatalf("Run() exit code = %d, want 0", code)
	}
	stdout := out.String()
	for _, want := range []string{
		"Discovered 2 MCP server(s)",
		"alpha (hermes/stdio) → node server.js",
		"beta (opencode/http) → https://mcp.example.com/sse",
	} {
		if !strings.Contains(stdout, want) {
			t.Errorf("stdout missing %q:\n%s", want, stdout)
		}
	}
	if strings.Contains(stdout, "supersecret-value") {
		t.Error("stdout leaked an env value")
	}
	if strings.Contains(stdout, "config file not found") {
		t.Error("findings leaked to stdout")
	}
	stderr := errOut.String()
	if !strings.Contains(stderr, "1 finding(s)") {
		t.Errorf("stderr missing finding count:\n%s", stderr)
	}
	if !strings.Contains(stderr, "unsupported [cursor] /home/test/.cursor/mcp.json: config file not found") {
		t.Errorf("stderr missing finding detail:\n%s", stderr)
	}
}

func TestRun_JSON(t *testing.T) {
	withFixture(t)
	var out, errOut bytes.Buffer
	code := Run([]string{"--format=json"}, &out, &errOut)
	if code != 0 {
		t.Fatalf("Run() exit code = %d, want 0", code)
	}
	var decoded struct {
		Servers []struct {
			Name    string            `json:"name"`
			Client  string            `json:"client"`
			Command string            `json:"command"`
			Env     map[string]string `json:"env"`
		} `json:"servers"`
	}
	if err := json.Unmarshal(out.Bytes(), &decoded); err != nil {
		t.Fatalf("stdout is not valid JSON: %v\n%s", err, out.String())
	}
	if len(decoded.Servers) != 2 {
		t.Fatalf("got %d servers, want 2", len(decoded.Servers))
	}
	// Views are sorted by client then name: alpha (hermes) before beta (opencode).
	if decoded.Servers[0].Name != "alpha" {
		t.Errorf("first server = %q, want alpha", decoded.Servers[0].Name)
	}
	if got := decoded.Servers[0].Env["TOKEN"]; got != "REDACTED" {
		t.Errorf("env TOKEN = %q, want REDACTED", got)
	}
	if strings.Contains(out.String(), "supersecret-value") {
		t.Error("stdout leaked an env value")
	}
	if strings.Contains(out.String(), "finding") {
		t.Error("findings leaked to stdout")
	}
	if !strings.Contains(errOut.String(), "unsupported") {
		t.Error("stderr missing finding")
	}
}

func TestRun_NoServers(t *testing.T) {
	orig := discoverAll
	discoverAll = func() discovery.Result { return discovery.Result{} }
	t.Cleanup(func() { discoverAll = orig })

	var out, errOut bytes.Buffer
	code := Run([]string{"--format", "table"}, &out, &errOut)
	if code != 0 {
		t.Fatalf("Run() exit code = %d, want 0", code)
	}
	if !strings.Contains(out.String(), "Discovered 0 MCP server(s)") {
		t.Errorf("unexpected stdout: %q", out.String())
	}
	if errOut.Len() != 0 {
		t.Errorf("expected empty stderr, got %q", errOut.String())
	}
}

func TestRun_NeverPrintsEnvValues(t *testing.T) {
	orig := discoverAll
	discoverAll = func() discovery.Result {
		return discovery.Result{
			Servers: []discovery.Server{
				{
					Name:      "s1",
					Client:    discovery.ClientHermes,
					Command:   "run",
					EnvKeys:   []string{"API_KEY", "SECRET"},
					EnvValues: []string{"hunter2", "opensesame"},
				},
			},
		}
	}
	t.Cleanup(func() { discoverAll = orig })

	for _, format := range []string{"table", "json"} {
		t.Run(format, func(t *testing.T) {
			var out, errOut bytes.Buffer
			code := Run([]string{"--format", format}, &out, &errOut)
			if code != 0 {
				t.Fatalf("Run() exit code = %d, want 0", code)
			}
			for _, secret := range []string{"hunter2", "opensesame"} {
				if strings.Contains(out.String(), secret) {
					t.Errorf("stdout leaked %q", secret)
				}
				if strings.Contains(errOut.String(), secret) {
					t.Errorf("stderr leaked %q", secret)
				}
			}
		})
	}
}

func TestRun_UnknownFlag(t *testing.T) {
	var out, errOut bytes.Buffer
	code := Run([]string{"--bogus"}, &out, &errOut)
	if code != 1 {
		t.Fatalf("Run() exit code = %d, want 1", code)
	}
	if !strings.Contains(errOut.String(), "unknown argument") {
		t.Errorf("stderr missing error, got: %q", errOut.String())
	}
}

func TestRun_FormatRequiresValue(t *testing.T) {
	var out, errOut bytes.Buffer
	code := Run([]string{"--format"}, &out, &errOut)
	if code != 1 {
		t.Fatalf("Run() exit code = %d, want 1", code)
	}
	if !strings.Contains(errOut.String(), "--format requires a value") {
		t.Errorf("stderr missing error, got: %q", errOut.String())
	}
}

func TestRun_UnsupportedFormat(t *testing.T) {
	var out, errOut bytes.Buffer
	code := Run([]string{"--format", "xml"}, &out, &errOut)
	if code != 1 {
		t.Fatalf("Run() exit code = %d, want 1", code)
	}
	if !strings.Contains(errOut.String(), "unsupported format") {
		t.Errorf("stderr missing error, got: %q", errOut.String())
	}
}

func TestRun_Help(t *testing.T) {
	var out, errOut bytes.Buffer
	code := Run([]string{"--help"}, &out, &errOut)
	if code != 0 {
		t.Fatalf("Run() exit code = %d, want 0", code)
	}
	if !strings.Contains(out.String(), "Usage:") {
		t.Errorf("stdout missing usage, got: %q", out.String())
	}
}
