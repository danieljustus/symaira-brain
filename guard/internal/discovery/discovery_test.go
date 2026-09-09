package discovery

import (
	"encoding/json"
	"os"
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
