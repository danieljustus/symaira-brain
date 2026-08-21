package discovery

import (
	"os"
	"path/filepath"
	"testing"
)

// ---------------------------------------------------------------------------
// Tests: ScanAllWithFS findings
// ---------------------------------------------------------------------------

func TestScanAllWithFS_MissingFiles(t *testing.T) {
	res := ScanAllWithFS(&testFS{files: map[string][]byte{}})
	if len(res.Servers) != 0 {
		t.Errorf("got %d servers, want 0", len(res.Servers))
	}
	if len(res.Findings) != 5 {
		t.Fatalf("got %d findings, want 5", len(res.Findings))
	}
	for _, f := range res.Findings {
		if f.Status != StatusUnsupported {
			t.Errorf("finding status = %q, want unsupported", f.Status)
		}
		if f.Client == "" {
			t.Error("finding has empty client")
		}
		if f.Path == "" {
			t.Error("finding has empty source path")
		}
		if f.Message == "" {
			t.Error("finding has empty message")
		}
	}
}

func TestScanAllWithFS_Mixed(t *testing.T) {
	sources := clientSources()
	files := map[string][]byte{}
	for _, src := range sources {
		if src.Client == ClientHermes {
			files[src.Path] = mustJSON(t, map[string]any{
				"mcpServers": map[string]any{"tool": map[string]any{"command": "echo"}},
			})
		}
	}
	res := ScanAllWithFS(&testFS{files: files})
	if len(res.Servers) != 1 {
		t.Fatalf("got %d servers, want 1", len(res.Servers))
	}
	if res.Servers[0].Name != "tool" {
		t.Errorf("server name = %q, want tool", res.Servers[0].Name)
	}
	if len(res.Findings) != 4 {
		t.Fatalf("got %d findings, want 4", len(res.Findings))
	}
	for _, f := range res.Findings {
		if f.Client == ClientHermes {
			t.Errorf("unexpected finding for present client %q", f.Client)
		}
	}
}

func TestScanAllWithFS_InvalidJSON(t *testing.T) {
	sources := clientSources()
	files := map[string][]byte{sources[0].Path: []byte(`{invalid json`)}
	res := ScanAllWithFS(&testFS{files: files})
	if len(res.Servers) != 0 {
		t.Errorf("got %d servers, want 0", len(res.Servers))
	}
	if len(res.Findings) != 5 {
		t.Fatalf("got %d findings, want 5", len(res.Findings))
	}
	var parseFinding *Finding
	for i, f := range res.Findings {
		if f.Path == sources[0].Path {
			parseFinding = &res.Findings[i]
		}
	}
	if parseFinding == nil {
		t.Fatal("no finding for the invalid config file")
	}
	if parseFinding.Status != StatusUnsupported {
		t.Errorf("status = %q, want unsupported", parseFinding.Status)
	}
	if parseFinding.Message == "" {
		t.Error("expected parse error message")
	}
}

// ---------------------------------------------------------------------------
// Tests: per-entry findings in the standard mcpServers format
// ---------------------------------------------------------------------------

func TestParseMCPserversFormat_UnmappableEntry(t *testing.T) {
	tests := []struct {
		name         string
		input        map[string]any
		wantServers  int
		wantFindings int
		wantStatus   Status
	}{
		{
			name:         "entry without command or url",
			input:        map[string]any{"mcpServers": map[string]any{"ghost": map[string]any{}}},
			wantServers:  0,
			wantFindings: 1,
			wantStatus:   StatusUnsupported,
		},
		{
			name:         "command entry maps exactly",
			input:        map[string]any{"mcpServers": map[string]any{"ok": map[string]any{"command": "echo"}}},
			wantServers:  1,
			wantFindings: 0,
		},
		{
			name:         "url entry maps exactly",
			input:        map[string]any{"mcpServers": map[string]any{"ok": map[string]any{"url": "https://mcp.example.com"}}},
			wantServers:  1,
			wantFindings: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			servers, findings, err := parseMCPserversFormat(ClientHermes, "/cfg/hermes.json", mustJSON(t, tt.input))
			if err != nil {
				t.Fatalf("parseMCPserversFormat() error = %v", err)
			}
			if len(servers) != tt.wantServers {
				t.Errorf("got %d servers, want %d", len(servers), tt.wantServers)
			}
			if len(findings) != tt.wantFindings {
				t.Fatalf("got %d findings, want %d", len(findings), tt.wantFindings)
			}
			if tt.wantFindings == 1 {
				f := findings[0]
				if f.Status != tt.wantStatus {
					t.Errorf("status = %q, want %q", f.Status, tt.wantStatus)
				}
				if f.Client != ClientHermes {
					t.Errorf("client = %q, want hermes", f.Client)
				}
				if f.Path != "/cfg/hermes.json" {
					t.Errorf("path = %q, want /cfg/hermes.json", f.Path)
				}
				if f.Message == "" {
					t.Error("expected non-empty message")
				}
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Tests: per-entry findings in the OpenCode format
// ---------------------------------------------------------------------------

func TestParseOpenCodeFormat_Findings(t *testing.T) {
	tests := []struct {
		name         string
		input        map[string]any
		wantServers  int
		wantFindings int
		wantStatus   Status
	}{
		{
			name:         "remote without url",
			input:        map[string]any{"mcp": map[string]any{"r": map[string]any{"type": "remote"}}},
			wantServers:  0,
			wantFindings: 1,
			wantStatus:   StatusUnsupported,
		},
		{
			name:         "local without command",
			input:        map[string]any{"mcp": map[string]any{"l": map[string]any{"type": "local"}}},
			wantServers:  0,
			wantFindings: 1,
			wantStatus:   StatusUnsupported,
		},
		{
			name:         "unknown type is approximate",
			input:        map[string]any{"mcp": map[string]any{"u": map[string]any{"type": "weird", "command": "echo"}}},
			wantServers:  1,
			wantFindings: 1,
			wantStatus:   StatusApproximate,
		},
		{
			name:         "remote with url maps exactly",
			input:        map[string]any{"mcp": map[string]any{"r": map[string]any{"type": "remote", "url": "https://mcp.example.com"}}},
			wantServers:  1,
			wantFindings: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			servers, findings, err := parseOpenCodeFormat(ClientOpenCode, "/cfg/opencode.json", mustJSON(t, tt.input))
			if err != nil {
				t.Fatalf("parseOpenCodeFormat() error = %v", err)
			}
			if len(servers) != tt.wantServers {
				t.Errorf("got %d servers, want %d", len(servers), tt.wantServers)
			}
			if len(findings) != tt.wantFindings {
				t.Fatalf("got %d findings, want %d", len(findings), tt.wantFindings)
			}
			if tt.wantFindings == 1 {
				f := findings[0]
				if f.Status != tt.wantStatus {
					t.Errorf("status = %q, want %q", f.Status, tt.wantStatus)
				}
				if f.Path != "/cfg/opencode.json" {
					t.Errorf("path = %q, want /cfg/opencode.json", f.Path)
				}
				if f.Message == "" {
					t.Error("expected non-empty message")
				}
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Tests: ScanAll with the real filesystem
// ---------------------------------------------------------------------------

func TestScanAll_MissingFiles(t *testing.T) {
	t.Setenv("HOME", t.TempDir())
	res := ScanAll()
	if len(res.Servers) != 0 {
		t.Errorf("got %d servers, want 0", len(res.Servers))
	}
	if len(res.Findings) != 5 {
		t.Fatalf("got %d findings, want 5", len(res.Findings))
	}
}

func TestScanAll_WithRealFiles(t *testing.T) {
	home := t.TempDir()
	hermesDir := filepath.Join(home, ".config", "hermes")
	if err := os.MkdirAll(hermesDir, 0o755); err != nil {
		t.Fatalf("mkdir: %v", err)
	}
	hermesConfig := filepath.Join(hermesDir, "config.json")
	if err := os.WriteFile(hermesConfig, []byte(`{"mcpServers":{"hermes-tool":{"command":"hermes-cmd"}}}`), 0o644); err != nil {
		t.Fatalf("write config: %v", err)
	}
	t.Setenv("HOME", home)

	res := ScanAll()
	if len(res.Servers) != 1 {
		t.Fatalf("got %d servers, want 1", len(res.Servers))
	}
	if res.Servers[0].Name != "hermes-tool" {
		t.Errorf("server name = %q, want hermes-tool", res.Servers[0].Name)
	}
	if len(res.Findings) != 4 {
		t.Fatalf("got %d findings, want 4", len(res.Findings))
	}
}

// ---------------------------------------------------------------------------
// Tests: status vocabulary
// ---------------------------------------------------------------------------

func TestStatusValues(t *testing.T) {
	statuses := []Status{StatusExact, StatusApproximate, StatusManual, StatusUnsupported}
	wantStrings := []string{"exact", "approximate", "manual", "unsupported"}
	if len(statuses) != len(wantStrings) {
		t.Fatal("status table mismatch")
	}
	for i, s := range statuses {
		if string(s) != wantStrings[i] {
			t.Errorf("status %q != %q", s, wantStrings[i])
		}
	}
}
