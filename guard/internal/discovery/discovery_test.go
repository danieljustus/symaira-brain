package discovery

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

// testFS is an in-memory filesystem for unit tests.
type testFS struct {
	files map[string][]byte
}

func (t *testFS) ReadFile(name string) ([]byte, error) {
	data, ok := t.files[name]
	if !ok {
		return nil, &os.PathError{Op: "open", Path: name, Err: os.ErrNotExist}
	}
	return data, nil
}

// ---------------------------------------------------------------------------
// Helper: JSON marshal helper
// ---------------------------------------------------------------------------

func mustJSON(t *testing.T, v any) []byte {
	t.Helper()
	data, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("mustJSON: %v", err)
	}
	return data
}

// ---------------------------------------------------------------------------
// Tests: Hermes
// ---------------------------------------------------------------------------

func TestParseHermes(t *testing.T) {
	tests := []struct {
		name      string
		input     map[string]any
		wantCount int
		wantName  string
		wantCmd   string
		wantArgs  []string
		wantEnv   []string // env keys
		wantTrans Transport
	}{
		{
			name: "single stdio server",
			input: map[string]any{
				"mcpServers": map[string]any{
					"my-tool": map[string]any{
						"command": "node",
						"args":    []any{"server.js"},
						"env": map[string]any{
							"API_KEY": "secret123",
						},
					},
				},
			},
			wantCount: 1,
			wantName:  "my-tool",
			wantCmd:   "node",
			wantArgs:  []string{"server.js"},
			wantEnv:   []string{"API_KEY"},
			wantTrans: TransportStdio,
		},
		{
			name: "multiple servers",
			input: map[string]any{
				"mcpServers": map[string]any{
					"tool-a": map[string]any{"command": "a"},
					"tool-b": map[string]any{"command": "b"},
				},
			},
			wantCount: 2,
		},
		{
			name:      "empty config",
			input:     map[string]any{},
			wantCount: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			data := mustJSON(t, tt.input)
			servers, err := ParseClientWithFS(&testFS{files: map[string][]byte{"test.json": data}}, ClientHermes, "test.json")
			if err != nil {
				t.Fatalf("ParseClientWithFS() error = %v", err)
			}
			if len(servers) != tt.wantCount {
				t.Fatalf("got %d servers, want %d", len(servers), tt.wantCount)
			}
			if tt.wantCount == 1 {
				s := servers[0]
				if s.Name != tt.wantName {
					t.Errorf("Name = %q, want %q", s.Name, tt.wantName)
				}
				if s.Client != ClientHermes {
					t.Errorf("Client = %q, want %q", s.Client, ClientHermes)
				}
				if s.Command != tt.wantCmd {
					t.Errorf("Command = %q, want %q", s.Command, tt.wantCmd)
				}
				if s.Transport != tt.wantTrans {
					t.Errorf("Transport = %q, want %q", s.Transport, tt.wantTrans)
				}
				if len(s.EnvKeys) > 0 && len(tt.wantEnv) > 0 {
					if len(s.EnvKeys) != len(tt.wantEnv) {
						t.Errorf("EnvKeys len = %d, want %d", len(s.EnvKeys), len(tt.wantEnv))
					}
				}
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Tests: Claude Desktop
// ---------------------------------------------------------------------------

func TestParseClaudeDesktop(t *testing.T) {
	tests := []struct {
		name      string
		input     map[string]any
		wantCount int
		wantName  string
		wantCmd   string
		wantArgs  []string
		wantTrans Transport
	}{
		{
			name: "stdio server",
			input: map[string]any{
				"mcpServers": map[string]any{
					"filesystem": map[string]any{
						"command": "npx",
						"args":    []any{"-y", "@modelcontextprotocol/server-filesystem", "/tmp"},
					},
				},
			},
			wantCount: 1,
			wantName:  "filesystem",
			wantCmd:   "npx",
			wantArgs:  []string{"-y", "@modelcontextprotocol/server-filesystem", "/tmp"},
			wantTrans: TransportStdio,
		},
		{
			name: "HTTP server via url field",
			input: map[string]any{
				"mcpServers": map[string]any{
					"remote-api": map[string]any{
						"url": "http://localhost:8080/sse",
					},
				},
			},
			wantCount: 1,
			wantName:  "remote-api",
			wantCmd:   "http://localhost:8080/sse",
			wantTrans: TransportHTTP,
		},
		{
			name: "server with env vars",
			input: map[string]any{
				"mcpServers": map[string]any{
					"api-tool": map[string]any{
						"command": "python",
						"args":    []any{"server.py"},
						"env": map[string]any{
							"TOKEN":    "abc123",
							"ENDPOINT": "https://api.example.com",
						},
					},
				},
			},
			wantCount: 1,
			wantName:  "api-tool",
			wantCmd:   "python",
			wantTrans: TransportStdio,
		},
		{
			name: "top-level keys besides mcpServers are ignored",
			input: map[string]any{
				"mcpServers": map[string]any{
					"tool": map[string]any{"command": "echo"},
				},
				"preferences": map[string]any{"theme": "dark"},
			},
			wantCount: 1,
			wantName:  "tool",
			wantCmd:   "echo",
			wantTrans: TransportStdio,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			data := mustJSON(t, tt.input)
			servers, err := ParseClientWithFS(&testFS{files: map[string][]byte{"test.json": data}}, ClientClaudeDesktop, "test.json")
			if err != nil {
				t.Fatalf("ParseClientWithFS() error = %v", err)
			}
			if len(servers) != tt.wantCount {
				t.Fatalf("got %d servers, want %d", len(servers), tt.wantCount)
			}
			if tt.wantCount == 1 {
				s := servers[0]
				if s.Name != tt.wantName {
					t.Errorf("Name = %q, want %q", s.Name, tt.wantName)
				}
				if s.Client != ClientClaudeDesktop {
					t.Errorf("Client = %q, want %q", s.Client, ClientClaudeDesktop)
				}
				if s.Command != tt.wantCmd {
					t.Errorf("Command = %q, want %q", s.Command, tt.wantCmd)
				}
				if s.Transport != tt.wantTrans {
					t.Errorf("Transport = %q, want %q", s.Transport, tt.wantTrans)
				}
				if len(tt.wantArgs) > 0 {
					if len(s.Args) != len(tt.wantArgs) {
						t.Errorf("Args len = %d, want %d", len(s.Args), len(tt.wantArgs))
					}
					for i, a := range tt.wantArgs {
						if i < len(s.Args) && s.Args[i] != a {
							t.Errorf("Args[%d] = %q, want %q", i, s.Args[i], a)
						}
					}
				}
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Tests: Cursor
// ---------------------------------------------------------------------------

func TestParseCursor(t *testing.T) {
	tests := []struct {
		name      string
		input     map[string]any
		wantCount int
		wantName  string
		wantCmd   string
		wantTrans Transport
	}{
		{
			name: "single server",
			input: map[string]any{
				"mcpServers": map[string]any{
					"context7": map[string]any{
						"command": "npx",
						"args":    []any{"-y", "@upstash/context7-mcp@latest"},
					},
				},
			},
			wantCount: 1,
			wantName:  "context7",
			wantCmd:   "npx",
			wantTrans: TransportStdio,
		},
		{
			name:      "empty mcpServers",
			input:     map[string]any{"mcpServers": map[string]any{}},
			wantCount: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			data := mustJSON(t, tt.input)
			servers, err := ParseClientWithFS(&testFS{files: map[string][]byte{"test.json": data}}, ClientCursor, "test.json")
			if err != nil {
				t.Fatalf("ParseClientWithFS() error = %v", err)
			}
			if len(servers) != tt.wantCount {
				t.Fatalf("got %d servers, want %d", len(servers), tt.wantCount)
			}
			if tt.wantCount == 1 {
				s := servers[0]
				if s.Name != tt.wantName {
					t.Errorf("Name = %q, want %q", s.Name, tt.wantName)
				}
				if s.Client != ClientCursor {
					t.Errorf("Client = %q, want %q", s.Client, ClientCursor)
				}
				if s.Command != tt.wantCmd {
					t.Errorf("Command = %q, want %q", s.Command, tt.wantCmd)
				}
				if s.Transport != tt.wantTrans {
					t.Errorf("Transport = %q, want %q", s.Transport, tt.wantTrans)
				}
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Tests: VS Code (Cline/Roo/Continue)
// ---------------------------------------------------------------------------

func TestParseVSCode(t *testing.T) {
	tests := []struct {
		name      string
		input     map[string]any
		wantCount int
		wantName  string
		wantCmd   string
		wantTrans Transport
	}{
		{
			name: "stdio server",
			input: map[string]any{
				"mcpServers": map[string]any{
					"brave-search": map[string]any{
						"command": "npx",
						"args":    []any{"-y", "@anthropic/mcp-brave-search"},
						"env": map[string]any{
							"BRAVE_API_KEY": "bsk_test",
						},
					},
				},
			},
			wantCount: 1,
			wantName:  "brave-search",
			wantCmd:   "npx",
			wantTrans: TransportStdio,
		},
		{
			name: "HTTP server",
			input: map[string]any{
				"mcpServers": map[string]any{
					"my-api": map[string]any{
						"url": "https://mcp.example.com/sse",
					},
				},
			},
			wantCount: 1,
			wantName:  "my-api",
			wantCmd:   "https://mcp.example.com/sse",
			wantTrans: TransportHTTP,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			data := mustJSON(t, tt.input)
			servers, err := ParseClientWithFS(&testFS{files: map[string][]byte{"test.json": data}}, ClientVSCode, "test.json")
			if err != nil {
				t.Fatalf("ParseClientWithFS() error = %v", err)
			}
			if len(servers) != tt.wantCount {
				t.Fatalf("got %d servers, want %d", len(servers), tt.wantCount)
			}
			if tt.wantCount == 1 {
				s := servers[0]
				if s.Name != tt.wantName {
					t.Errorf("Name = %q, want %q", s.Name, tt.wantName)
				}
				if s.Client != ClientVSCode {
					t.Errorf("Client = %q, want %q", s.Client, ClientVSCode)
				}
				if s.Command != tt.wantCmd {
					t.Errorf("Command = %q, want %q", s.Command, tt.wantCmd)
				}
				if s.Transport != tt.wantTrans {
					t.Errorf("Transport = %q, want %q", s.Transport, tt.wantTrans)
				}
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Tests: OpenCode
// ---------------------------------------------------------------------------

func TestParseOpenCode(t *testing.T) {
	tests := []struct {
		name      string
		input     map[string]any
		wantCount int
		wantName  string
		wantCmd   string
		wantArgs  []string
		wantEnv   []string
		wantTrans Transport
	}{
		{
			name: "local server",
			input: map[string]any{
				"mcp": map[string]any{
					"context7": map[string]any{
						"type":    "local",
						"command": "npx",
						"args":    []any{"-y", "@upstash/context7-mcp@latest"},
						"environment": map[string]any{
							"MY_KEY": "value123",
						},
					},
				},
			},
			wantCount: 1,
			wantName:  "context7",
			wantCmd:   "npx",
			wantArgs:  []string{"-y", "@upstash/context7-mcp@latest"},
			wantEnv:   []string{"MY_KEY"},
			wantTrans: TransportStdio,
		},
		{
			name: "remote server",
			input: map[string]any{
				"mcp": map[string]any{
					"remote-svc": map[string]any{
						"type": "remote",
						"url":  "https://mcp.example.com/sse",
					},
				},
			},
			wantCount: 1,
			wantName:  "remote-svc",
			wantCmd:   "https://mcp.example.com/sse",
			wantTrans: TransportHTTP,
		},
		{
			name: "env key fallback",
			input: map[string]any{
				"mcp": map[string]any{
					"tool": map[string]any{
						"type":    "local",
						"command": "node",
						"args":    []any{"server.js"},
						"env": map[string]any{
							"FALLBACK_KEY": "val",
						},
					},
				},
			},
			wantCount: 1,
			wantName:  "tool",
			wantCmd:   "node",
			wantArgs:  []string{"server.js"},
			wantEnv:   []string{"FALLBACK_KEY"},
			wantTrans: TransportStdio,
		},
		{
			name:      "empty mcp",
			input:     map[string]any{},
			wantCount: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			data := mustJSON(t, tt.input)
			servers, err := ParseClientWithFS(&testFS{files: map[string][]byte{"test.json": data}}, ClientOpenCode, "test.json")
			if err != nil {
				t.Fatalf("ParseClientWithFS() error = %v", err)
			}
			if len(servers) != tt.wantCount {
				t.Fatalf("got %d servers, want %d", len(servers), tt.wantCount)
			}
			if tt.wantCount == 1 {
				s := servers[0]
				if s.Name != tt.wantName {
					t.Errorf("Name = %q, want %q", s.Name, tt.wantName)
				}
				if s.Client != ClientOpenCode {
					t.Errorf("Client = %q, want %q", s.Client, ClientOpenCode)
				}
				if s.Command != tt.wantCmd {
					t.Errorf("Command = %q, want %q", s.Command, tt.wantCmd)
				}
				if s.Transport != tt.wantTrans {
					t.Errorf("Transport = %q, want %q", s.Transport, tt.wantTrans)
				}
				if len(tt.wantArgs) > 0 && len(s.Args) != len(tt.wantArgs) {
					t.Errorf("Args len = %d, want %d", len(s.Args), len(tt.wantArgs))
				}
				if len(tt.wantEnv) > 0 && len(s.EnvKeys) != len(tt.wantEnv) {
					t.Errorf("EnvKeys len = %d, want %d", len(s.EnvKeys), len(tt.wantEnv))
				}
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Tests: Missing file handling
// ---------------------------------------------------------------------------

func TestParseClient_MissingFile(t *testing.T) {
	clients := []Client{ClientHermes, ClientClaudeDesktop, ClientCursor, ClientVSCode, ClientOpenCode}
	for _, client := range clients {
		t.Run(string(client), func(t *testing.T) {
			servers, err := ParseClientWithFS(&testFS{files: map[string][]byte{}}, client, "nonexistent.json")
			if err != nil {
				t.Fatalf("expected no error for missing file, got: %v", err)
			}
			if servers != nil {
				t.Fatalf("expected nil for missing file, got %d servers", len(servers))
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Tests: Invalid JSON
// ---------------------------------------------------------------------------

func TestParseClient_InvalidJSON(t *testing.T) {
	tests := []struct {
		name   string
		client Client
		data   string
	}{
		{"hermes", ClientHermes, `{invalid json`},
		{"claude-desktop", ClientClaudeDesktop, `not json`},
		{"cursor", ClientCursor, `[{`},
		{"vscode", ClientVSCode, `{broken`},
		{"opencode", ClientOpenCode, `}`},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			fsys := &testFS{files: map[string][]byte{"bad.json": []byte(tt.data)}}
			_, err := ParseClientWithFS(fsys, tt.client, "bad.json")
			if err == nil {
				t.Fatal("expected error for invalid JSON, got nil")
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Tests: Server.RedactedEnv and Server.String
// ---------------------------------------------------------------------------

func TestServer_RedactedEnv(t *testing.T) {
	s := Server{
		Name:      "test",
		Client:    ClientHermes,
		Command:   "node",
		EnvKeys:   []string{"API_KEY", "SECRET"},
		EnvValues: []string{"real_value", "top_secret"},
	}
	redacted := s.RedactedEnv()
	if len(redacted) != 2 {
		t.Fatalf("got %d env vars, want 2", len(redacted))
	}
	for k, v := range redacted {
		if v != "REDACTED" {
			t.Errorf("env %q = %q, want REDACTED", k, v)
		}
	}
}

func TestServer_String(t *testing.T) {
	s := Server{
		Name:      "my-tool",
		Client:    ClientHermes,
		Command:   "node",
		Args:      []string{"server.js"},
		Transport: TransportStdio,
	}
	got := s.String()
	want := "my-tool (hermes/stdio) → node server.js"
	if got != want {
		t.Errorf("String() = %q, want %q", got, want)
	}
}

// ---------------------------------------------------------------------------
// Tests: DiscoverAllWithFS (integration-style with test FS)
// ---------------------------------------------------------------------------

func TestDiscoverAllWithFS(t *testing.T) {
	// Build a test filesystem that has Hermes and Claude Desktop configs.
	hermes := map[string]any{
		"mcpServers": map[string]any{
			"hermes-tool": map[string]any{
				"command": "hermes-cmd",
			},
		},
	}
	claude := map[string]any{
		"mcpServers": map[string]any{
			"claude-tool": map[string]any{
				"command": "claude-cmd",
			},
		},
	}

	// Use the source paths directly so DiscoverAllWithFS can find them.
	sources := clientSources()
	files := map[string][]byte{}
	for _, src := range sources {
		switch src.Client {
		case ClientHermes:
			files[src.Path] = mustJSON(t, hermes)
		case ClientClaudeDesktop:
			files[src.Path] = mustJSON(t, claude)
		}
	}

	servers, err := DiscoverAllWithFS(&testFS{files: files})
	if err != nil {
		t.Fatalf("DiscoverAllWithFS() error = %v", err)
	}

	// Should find servers from both clients.
	clients := map[Client]bool{}
	for _, s := range servers {
		clients[s.Client] = true
	}
	if !clients[ClientHermes] {
		t.Error("expected Hermes servers in discovery results")
	}
	if !clients[ClientClaudeDesktop] {
		t.Error("expected Claude Desktop servers in discovery results")
	}
}

// ---------------------------------------------------------------------------
// Tests: clientSources coverage
// ---------------------------------------------------------------------------

func TestClientSources(t *testing.T) {
	sources := clientSources()
	if len(sources) != 5 {
		t.Fatalf("clientSources() returned %d sources, want 5", len(sources))
	}

	seen := map[Client]bool{}
	for _, src := range sources {
		if seen[src.Client] {
			t.Errorf("duplicate source for client %q", src.Client)
		}
		seen[src.Client] = true
		if src.Path == "" {
			t.Errorf("empty path for client %q", src.Client)
		}
	}
}

func TestClientSourcesForGOOS_Darwin(t *testing.T) {
	sources := clientSourcesForGOOS("darwin", "/Users/test")
	if len(sources) != 5 {
		t.Fatalf("got %d sources, want 5", len(sources))
	}

	var claudeSource *clientSource
	for i, src := range sources {
		if src.Client == ClientClaudeDesktop {
			claudeSource = &sources[i]
			break
		}
	}
	if claudeSource == nil {
		t.Fatal("no Claude Desktop source found")
	}
	want := filepath.Join("/Users/test", "Library", "Application Support", "Claude", "claude_desktop_config.json")
	if claudeSource.Path != want {
		t.Errorf("Claude Desktop path = %q, want %q", claudeSource.Path, want)
	}
}

func TestClientSourcesForGOOS_Linux(t *testing.T) {
	t.Setenv("XDG_CONFIG_HOME", "")
	sources := clientSourcesForGOOS("linux", "/home/test")
	if len(sources) != 5 {
		t.Fatalf("got %d sources, want 5", len(sources))
	}

	var claudeSource *clientSource
	for i, src := range sources {
		if src.Client == ClientClaudeDesktop {
			claudeSource = &sources[i]
			break
		}
	}
	if claudeSource == nil {
		t.Fatal("no Claude Desktop source found")
	}
	want := filepath.Join("/home/test", ".config", "claude", "claude_desktop_config.json")
	if claudeSource.Path != want {
		t.Errorf("Claude Desktop path = %q, want %q", claudeSource.Path, want)
	}
}

func TestClientSourcesForGOOS_Linux_XDG(t *testing.T) {
	t.Setenv("XDG_CONFIG_HOME", "/custom/xdg")
	sources := clientSourcesForGOOS("linux", "/home/test")

	var claudeSource *clientSource
	for i, src := range sources {
		if src.Client == ClientClaudeDesktop {
			claudeSource = &sources[i]
			break
		}
	}
	if claudeSource == nil {
		t.Fatal("no Claude Desktop source found")
	}
	want := filepath.Join("/custom/xdg", "claude", "claude_desktop_config.json")
	if claudeSource.Path != want {
		t.Errorf("Claude Desktop path = %q, want %q", claudeSource.Path, want)
	}
}

// ---------------------------------------------------------------------------
// Tests: Unsupported client
// ---------------------------------------------------------------------------

func TestParseClient_Unsupported(t *testing.T) {
	fsys := &testFS{files: map[string][]byte{"test.json": []byte(`{}`)}}
	_, err := ParseClientWithFS(fsys, "unknown-client", "test.json")
	if err == nil {
		t.Fatal("expected error for unsupported client, got nil")
	}
}

// ---------------------------------------------------------------------------
// Tests: DiscoverAll() — integration-style wrapper coverage
// ---------------------------------------------------------------------------

func TestDiscoverAll_MissingFiles(t *testing.T) {
	// DiscoverAll() with no config files present should return empty, not error.
	// We can't easily control all file paths without env overrides, but we can
	// verify it doesn't crash when run in a clean environment.
	servers, err := DiscoverAll()
	if err != nil {
		t.Fatalf("DiscoverAll() error = %v", err)
	}
	_ = servers
}

func TestDiscoverAll_WithRealFiles(t *testing.T) {
	home := t.TempDir()
	hermesDir := filepath.Join(home, ".config", "hermes")
	if err := os.MkdirAll(hermesDir, 0o755); err != nil {
		t.Fatalf("mkdir: %v", err)
	}
	hermesConfig := filepath.Join(hermesDir, "config.json")
	hermesData := []byte(`{"mcpServers":{"hermes-tool":{"command":"hermes-cmd"}}}`)
	if err := os.WriteFile(hermesConfig, hermesData, 0o644); err != nil {
		t.Fatalf("write config: %v", err)
	}

	origHome := os.Getenv("HOME")
	t.Setenv("HOME", home)
	t.Cleanup(func() { os.Setenv("HOME", origHome) })

	servers, err := DiscoverAll()
	if err != nil {
		t.Fatalf("DiscoverAll() error = %v", err)
	}

	found := false
	for _, s := range servers {
		if s.Client == ClientHermes && s.Name == "hermes-tool" {
			found = true
			break
		}
	}
	if !found {
		t.Error("DiscoverAll() did not find hermes-tool from real config file")
	}
}

func TestParseClient_OS_MissingFile(t *testing.T) {
	servers, err := ParseClient(ClientHermes, filepath.Join(t.TempDir(), "nonexistent.json"))
	if err != nil {
		t.Fatalf("ParseClient() missing file: unexpected error: %v", err)
	}
	if servers != nil {
		t.Fatalf("ParseClient() missing file: got %d servers, want nil", len(servers))
	}
}

func TestParseClient_OS_ValidFile(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "config.json")
	data := []byte(`{"mcpServers":{"my-tool":{"command":"node","args":["server.js"]}}}`)
	if err := os.WriteFile(path, data, 0o644); err != nil {
		t.Fatalf("write config: %v", err)
	}

	servers, err := ParseClient(ClientHermes, path)
	if err != nil {
		t.Fatalf("ParseClient() valid file: unexpected error: %v", err)
	}
	if len(servers) != 1 {
		t.Fatalf("got %d servers, want 1", len(servers))
	}
	if servers[0].Name != "my-tool" {
		t.Errorf("Name = %q, want %q", servers[0].Name, "my-tool")
	}
	if servers[0].Command != "node" {
		t.Errorf("Command = %q, want %q", servers[0].Command, "node")
	}
}

func TestParseClient_OS_InvalidJSON(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "bad.json")
	if err := os.WriteFile(path, []byte(`{invalid`), 0o644); err != nil {
		t.Fatalf("write config: %v", err)
	}

	_, err := ParseClient(ClientHermes, path)
	if err == nil {
		t.Fatal("ParseClient() invalid JSON: expected error, got nil")
	}
}

// ---------------------------------------------------------------------------
// Tests: Transport type detection
// ---------------------------------------------------------------------------

func TestTransportDetection(t *testing.T) {
	tests := []struct {
		name         string
		server       rawServer
		wantTrans    Transport
		wantCmd      string
		wantFindings int
	}{
		{
			name:         "command → stdio",
			server:       rawServer{Command: "node", Args: []string{"s.js"}},
			wantTrans:    TransportStdio,
			wantCmd:      "node",
			wantFindings: 0,
		},
		{
			name:         "url → http",
			server:       rawServer{URL: "http://localhost:8080/sse"},
			wantTrans:    TransportHTTP,
			wantCmd:      "http://localhost:8080/sse",
			wantFindings: 0,
		},
		{
			name:         "url + command → http, command preserved",
			server:       rawServer{Command: "npx", URL: "http://localhost:8080/sse"},
			wantTrans:    TransportHTTP,
			wantCmd:      "npx",
			wantFindings: 0,
		},
		{
			name:         "neither command nor url → unsupported finding",
			server:       rawServer{},
			wantFindings: 1,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			raw := map[string]rawServer{"test": tt.server}
			servers, findings := rawServersToServers(ClientCursor, "test.json", raw)
			if len(servers) != 1-tt.wantFindings {
				t.Fatalf("got %d servers, want %d", len(servers), 1-tt.wantFindings)
			}
			if len(findings) != tt.wantFindings {
				t.Fatalf("got %d findings, want %d", len(findings), tt.wantFindings)
			}
			if tt.wantFindings == 1 {
				if findings[0].Status != StatusUnsupported {
					t.Errorf("finding status = %q, want unsupported", findings[0].Status)
				}
				if findings[0].Path != "test.json" {
					t.Errorf("finding path = %q, want test.json", findings[0].Path)
				}
				return
			}
			s := servers[0]
			if s.Transport != tt.wantTrans {
				t.Errorf("Transport = %q, want %q", s.Transport, tt.wantTrans)
			}
			if s.Command != tt.wantCmd {
				t.Errorf("Command = %q, want %q", s.Command, tt.wantCmd)
			}
		})
	}
}
