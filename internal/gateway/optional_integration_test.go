package gateway

import (
	"context"
	"encoding/json"
	"log/slog"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/profile"
)

func TestServeIO_OptionalModulesListAndCallOnlyBoundTools(t *testing.T) {
	p := testProfile()
	p.Servers[profile.ServerOperate] = profile.ServerConfig{
		Enabled: true, ToolsAllow: []string{"version", "permissions_status"},
	}
	p.Servers[profile.ServerScope] = profile.ServerConfig{
		Enabled: true, ToolsAllow: []string{"scan", "ports_list"},
	}
	operate := newManagedFake(t, profile.ServerOperate, `[{"name":"version"},{"name":"permissions_status"},{"name":"not_allowlisted"}]`)
	scope := newManagedFake(t, profile.ServerScope, `[{"name":"scan"},{"name":"ports_list"},{"name":"scope_not_allowlisted"}]`)
	s := New(p, map[string]*broker.ManagedServer{
		profile.ServerOperate: operate,
		profile.ServerScope:   scope,
	}, slog.Default(), nil, "dev")

	ctx, preCancel := context.WithTimeout(context.Background(), 10*time.Second)
	if err := s.buildCatalog(ctx); err != nil {
		preCancel()
		t.Fatalf("buildCatalog: %v", err)
	}
	preCancel()
	sr, sw, cr, cw := bidirectionalPipe(t)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	go func() { _ = s.ServeIO(ctx, sr, sw) }()

	writeJSON(t, cw, initializeRequest(1))
	if response := readJSONResponse(t, cr); response.Error != nil {
		t.Fatalf("initialize error: %v", response.Error)
	}
	writeJSON(t, cw, map[string]any{"jsonrpc": "2.0", "id": float64(2), "method": "tools/list"})
	list := readJSONResponse(t, cr)
	if list.Error != nil {
		t.Fatalf("tools/list error: %v", list.Error)
	}
	var listed struct {
		Tools []struct {
			Name string `json:"name"`
		} `json:"tools"`
	}
	if err := json.Unmarshal(list.Result, &listed); err != nil {
		t.Fatalf("decode tools/list: %v", err)
	}
	names := map[string]bool{}
	for _, tool := range listed.Tools {
		names[tool.Name] = true
	}
	for _, name := range []string{"version", "permissions_status", "scan", "ports_list"} {
		if !names[name] {
			t.Errorf("tools/list missing opted-in tool %q", name)
		}
	}
	for _, name := range []string{"not_allowlisted", "scope_not_allowlisted"} {
		if names[name] {
			t.Errorf("tools/list exposed out-of-universe tool %q", name)
		}
	}

	for id, name := range []string{"version", "scan"} {
		writeJSON(t, cw, map[string]any{
			"jsonrpc": "2.0", "id": float64(id + 3), "method": "tools/call",
			"params": map[string]any{"name": name, "arguments": map[string]any{}},
		})
		response := readJSONResponse(t, cr)
		if response.Error != nil {
			t.Fatalf("tools/call %s error: %v", name, response.Error)
		}
	}
	writeJSON(t, cw, map[string]any{
		"jsonrpc": "2.0", "id": float64(5), "method": "tools/call",
		"params": map[string]any{"name": "not_allowlisted", "arguments": map[string]any{}},
	})
	denied := readJSONResponse(t, cr)
	if denied.Error == nil || denied.Error.Code != -32601 {
		t.Fatalf("out-of-universe call error = %+v, want JSON-RPC -32601", denied.Error)
	}
}

func TestServeIO_OptionalModulesEnabledWithoutAllowlistRemainPrivate(t *testing.T) {
	p := testProfile()
	p.Servers[profile.ServerOperate] = profile.ServerConfig{Enabled: true}
	operate := newManagedFake(t, profile.ServerOperate, `[{"name":"version"}]`)
	s := New(p, map[string]*broker.ManagedServer{profile.ServerOperate: operate}, slog.Default(), nil, "dev")
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := s.buildCatalog(ctx); err != nil {
		t.Fatalf("buildCatalog: %v", err)
	}
	for _, entry := range s.cat.Exposed() {
		if entry.Server == profile.ServerOperate {
			t.Fatalf("enabled optional module exposed tool without tools_allow: %q", entry.Name)
		}
	}
}

func TestServeIO_OptionalModulesDefaultDisabledEvenWithLiveChildren(t *testing.T) {
	// The live child map may contain optional servers, but the default profile
	// must still omit them from the public handler and never query/expose them.
	s := New(testProfile(), map[string]*broker.ManagedServer{
		profile.ServerOperate: newManagedFake(t, profile.ServerOperate, `[{"name":"version"}]`),
		profile.ServerScope:   newManagedFake(t, profile.ServerScope, `[{"name":"scan"}]`),
	}, slog.Default(), nil, "dev")
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := s.buildCatalog(ctx); err != nil {
		t.Fatalf("buildCatalog: %v", err)
	}
	for _, entry := range s.cat.Exposed() {
		if entry.Server == profile.ServerOperate || entry.Server == profile.ServerScope {
			t.Fatalf("default profile exposed disabled optional tool %q", entry.Name)
		}
	}
}
