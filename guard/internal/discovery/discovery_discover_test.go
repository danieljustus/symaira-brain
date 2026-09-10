package discovery

import (
	"os"
	"path/filepath"
	"testing"
)

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
		config       string
		wantTrans    Transport
		wantCmd      string
		wantFindings int
	}{
		{
			name:         "command → stdio",
			config:       `{"mcpServers": {"test": {"command": "node", "args": ["s.js"]}}}`,
			wantTrans:    TransportStdio,
			wantCmd:      "node",
			wantFindings: 0,
		},
		{
			name:         "url → http",
			config:       `{"mcpServers": {"test": {"url": "http://localhost:8080/sse"}}}`,
			wantTrans:    TransportHTTP,
			wantCmd:      "http://localhost:8080/sse",
			wantFindings: 0,
		},
		{
			name:         "url + command → http, command preserved",
			config:       `{"mcpServers": {"test": {"command": "npx", "url": "http://localhost:8080/sse"}}}`,
			wantTrans:    TransportHTTP,
			wantCmd:      "npx",
			wantFindings: 0,
		},
		{
			name:         "neither command nor url → unsupported finding",
			config:       `{"mcpServers": {"test": {}}}`,
			wantFindings: 1,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			fsys := &testFS{files: map[string][]byte{"test.json": []byte(tt.config)}}
			servers, err := ParseClientWithFS(fsys, ClientCursor, "test.json")
			if tt.wantFindings == 1 {
				if err == nil {
					t.Fatalf("expected error for unmappable entry, got nil")
				}
				return
			}
			if err != nil {
				t.Fatalf("ParseClientWithFS() error = %v", err)
			}
			if len(servers) != 1 {
				t.Fatalf("got %d servers, want 1", len(servers))
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
