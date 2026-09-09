package discovery

import "testing"

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
