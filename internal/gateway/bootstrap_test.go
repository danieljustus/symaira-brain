package gateway

import (
	"context"
	"encoding/json"
	"log/slog"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/catalog"
	"github.com/danieljustus/symaira-brain/internal/policy"
	"github.com/danieljustus/symaira-brain/internal/profile"
)

// --- direct handler tests ---

// serverWithCatalog builds a Server whose catalog is pre-populated from
// the given per-server tool lists (bypassing live children, since the
// bootstrap handler only reads in-memory state).
func serverWithCatalog(t *testing.T, p *profile.Profile, perServerTools map[string][]string) *Server {
	t.Helper()

	servers := make(map[string]*broker.ManagedServer, len(perServerTools))
	var serverTools []catalog.ServerTools

	for alias, toolNames := range perServerTools {
		// A placeholder managed server so vaultStatus sees the server as
		// reachable; it is never called by the bootstrap handler.
		servers[alias] = broker.NewManagedServer(broker.ServerConfig{Name: alias})

		tools := make([]catalog.Tool, len(toolNames))
		for i, name := range toolNames {
			tools[i] = catalog.Tool{Name: name, Description: "test"}
		}

		report, err := policy.Evaluate(alias, p.Server(alias), toolNames)
		if err != nil {
			t.Fatalf("policy.Evaluate(%s): %v", alias, err)
		}
		serverTools = append(serverTools, catalog.ServerTools{
			Server: alias,
			Tools:  tools,
			Report: report,
		})
	}

	cat, err := catalog.Build(serverTools)
	if err != nil {
		t.Fatalf("catalog.Build: %v", err)
	}

	s := New(p, servers, slog.Default(), nil, "dev")
	s.cat = cat
	return s
}

func callBootstrap(t *testing.T, s *Server) bootstrapResponse {
	t.Helper()
	raw, err := s.handleBootstrap(context.Background(), json.RawMessage(`{}`))
	if err != nil {
		t.Fatalf("handleBootstrap: %v", err)
	}
	data, err := json.Marshal(raw)
	if err != nil {
		t.Fatalf("marshal bootstrap payload: %v", err)
	}
	var resp bootstrapResponse
	if err := json.Unmarshal(data, &resp); err != nil {
		t.Fatalf("unmarshal bootstrap payload: %v", err)
	}
	return resp
}

func TestBootstrap_ExposureSummary(t *testing.T) {
	p := testProfile()
	s := serverWithCatalog(t, p, map[string][]string{
		"vault":  {"get_entry", "health"},
		"memory": {"memory_search", "entity_list"},
	})

	resp := callBootstrap(t, s)

	if resp.Profile != "test" {
		t.Errorf("profile = %q, want %q", resp.Profile, "test")
	}
	if resp.GeneratedAt == "" {
		t.Error("generated_at should be set")
	}
	if _, err := time.Parse(time.RFC3339, resp.GeneratedAt); err != nil {
		t.Errorf("generated_at %q is not RFC3339: %v", resp.GeneratedAt, err)
	}

	// Catalog: namespaced exposed tool names, sorted; raw child names
	// never leak into the exposed list.
	got := strings.Join(resp.Catalog, ",")
	for _, want := range []string{"vault_get_entry", "vault_health", "memory_search", "entity_list"} {
		if !strings.Contains(got, want) {
			t.Errorf("catalog should contain %q, got %q", want, got)
		}
	}
	exact := make(map[string]bool, len(resp.Catalog))
	for _, name := range resp.Catalog {
		exact[name] = true
	}
	if exact["get_entry"] {
		t.Errorf("catalog leaks the unprefixed child tool name %q", "get_entry")
	}

	// Per-server exposure.
	byServer := make(map[string]bootstrapServer)
	for _, srv := range resp.Servers {
		byServer[srv.Server] = srv
	}
	vault, ok := byServer["vault"]
	if !ok {
		t.Fatal("servers should include vault")
	}
	if !vault.Enabled || vault.Mode != profile.VaultModeFull {
		t.Errorf("vault enabled=%v mode=%q, want enabled mode=%q", vault.Enabled, vault.Mode, profile.VaultModeFull)
	}
	if vault.ExposedCount != 2 || len(vault.ExposedTools) != 2 {
		t.Errorf("vault exposed_count=%d exposed_tools=%v, want 2 tools", vault.ExposedCount, vault.ExposedTools)
	}
	mem := byServer["memory"]
	if mem.ExposedCount != 2 || len(mem.ExposedTools) != 2 {
		t.Errorf("memory exposed_count=%d, want 2", mem.ExposedCount)
	}

	if resp.Vault.Status != "present" {
		t.Errorf("vault.status = %q, want %q", resp.Vault.Status, "present")
	}
	if resp.Vault.Listing != "unavailable-without-unlock" {
		t.Errorf("vault.listing = %q, want %q", resp.Vault.Listing, "unavailable-without-unlock")
	}
}

func TestBootstrap_VaultOffAndDisabled(t *testing.T) {
	for _, mode := range []string{profile.VaultModeOff, ""} {
		p := testProfile()
		if mode == "" {
			p.Servers["vault"] = profile.ServerConfig{Enabled: false}
		} else {
			p.Servers["vault"] = profile.ServerConfig{Enabled: true, Mode: mode}
		}
		s := serverWithCatalog(t, p, map[string][]string{"memory": {"memory_search"}})

		resp := callBootstrap(t, s)
		if resp.Vault.Status != "disabled" {
			t.Errorf("mode %q: vault.status = %q, want %q", mode, resp.Vault.Status, "disabled")
		}
	}
}

func TestBootstrap_VaultAbsent(t *testing.T) {
	p := testProfile()
	// Vault is enabled in the profile but never made it into the live
	// server set (e.g. binary not found at spawn).
	s := serverWithCatalog(t, p, map[string][]string{"memory": {"memory_search"}})
	delete(s.servers, "vault")

	resp := callBootstrap(t, s)
	if resp.Vault.Status != "absent" {
		t.Errorf("vault.status = %q, want %q", resp.Vault.Status, "absent")
	}
}

func TestBootstrap_SkillsDisabled(t *testing.T) {
	p := testProfile()
	p.Servers["skills"] = profile.ServerConfig{Enabled: false}
	s := serverWithCatalog(t, p, map[string][]string{
		"vault":  {"health"},
		"memory": {"memory_search"},
	})

	resp := callBootstrap(t, s)

	byServer := make(map[string]bootstrapServer)
	for _, srv := range resp.Servers {
		byServer[srv.Server] = srv
	}
	skills := byServer["skills"]
	if skills.Enabled {
		t.Error("skills should be disabled")
	}
	if skills.ExposedCount != 0 || len(skills.ExposedTools) != 0 {
		t.Errorf("skills exposed_count=%d, want 0", skills.ExposedCount)
	}
}

// --- end-to-end ServeIO test ---

func TestBootstrap_OverServeIO(t *testing.T) {
	vault := newManagedFake(t, "vault",
		`[{"name":"get_entry","description":"fetch secret"},{"name":"health","description":"healthcheck"}]`)
	memory := newManagedFake(t, "memory",
		`[{"name":"memory_search","description":"search memories"},{"name":"entity_list","description":"list entities"}]`)

	s := New(testProfile(), map[string]*broker.ManagedServer{
		"vault":  vault,
		"memory": memory,
	}, slog.Default(), nil, "dev")

	sr, sw, cr, cw := bidirectionalPipe(t)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	go func() { _ = s.ServeIO(ctx, sr, sw) }()

	writeJSON(t, cw, initializeRequest(1))
	if resp := readJSONResponse(t, cr); resp.Error != nil {
		t.Fatalf("initialize error: %v", resp.Error)
	}

	writeJSON(t, cw, toolsCallRequest(2, "bootstrap"))
	resp := readJSONResponse(t, cr)
	if resp.Error != nil {
		t.Fatalf("tools/call bootstrap error: %v", resp.Error)
	}

	// The MCP result wraps the payload in a text content block.
	var toolResult struct {
		Content []struct {
			Text string `json:"text"`
		} `json:"content"`
	}
	if err := json.Unmarshal(resp.Result, &toolResult); err != nil {
		t.Fatalf("unmarshal tools/call result: %v", err)
	}
	if len(toolResult.Content) != 1 || toolResult.Content[0].Text == "" {
		t.Fatalf("expected one text content block, got %+v", toolResult.Content)
	}

	var payload bootstrapResponse
	if err := json.Unmarshal([]byte(toolResult.Content[0].Text), &payload); err != nil {
		t.Fatalf("unmarshal bootstrap payload: %v\nraw: %s", err, toolResult.Content[0].Text)
	}
	if payload.Profile != "test" {
		t.Errorf("profile = %q, want %q", payload.Profile, "test")
	}
	joined := strings.Join(payload.Catalog, ",")
	for _, want := range []string{"vault_get_entry", "vault_health", "memory_search", "entity_list"} {
		if !strings.Contains(joined, want) {
			t.Errorf("catalog should contain %q, got %q", want, joined)
		}
	}
	if payload.Vault.Status != "present" {
		t.Errorf("vault.status = %q, want %q", payload.Vault.Status, "present")
	}
}
