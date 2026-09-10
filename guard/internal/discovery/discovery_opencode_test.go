package discovery

import "testing"

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
