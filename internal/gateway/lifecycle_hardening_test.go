package gateway

import (
	"context"
	"encoding/json"
	"log/slog"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/profile"
)

func TestServeIO_OptionalModulesExposeExactAllowlistsAndDenyBeforeChild(t *testing.T) {
	callMarker := filepath.Join(t.TempDir(), "calls.log")
	p := &profile.Profile{Name: "optional", Servers: profile.Servers{
		profile.ServerOperate: {Enabled: true, ToolsAllow: []string{"version", "permissions_status", "get_policy"}},
		profile.ServerScope:   {Enabled: true, ToolsAllow: []string{"scan", "ports_list", "ports_suggest", "mcp_list", "conflicts", "mcp_health", "daemons_list"}},
	}}
	operate := managedOptionalFake(t, profile.ServerOperate, callMarker, `[{"name":"version"},{"name":"permissions_status"},{"name":"get_policy"},{"name":"operate_secret"}]`)
	scope := managedOptionalFake(t, profile.ServerScope, callMarker, `[{"name":"scan"},{"name":"ports_list"},{"name":"ports_suggest"},{"name":"mcp_list"},{"name":"conflicts"},{"name":"mcp_health"},{"name":"daemons_list"},{"name":"scope_secret"}]`)
	server := New(p, map[string]*broker.ManagedServer{profile.ServerOperate: operate, profile.ServerScope: scope}, slog.Default(), nil, "test")

	sr, sw, cr, cw := bidirectionalPipe(t)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	go func() { _ = server.ServeIO(ctx, sr, sw) }()
	writeJSON(t, cw, initializeRequest(1))
	if response := readJSONResponse(t, cr); response.Error != nil {
		t.Fatalf("initialize: %v", response.Error)
	}
	writeJSON(t, cw, map[string]any{"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
	response := readJSONResponse(t, cr)
	if response.Error != nil {
		t.Fatalf("tools/list: %v", response.Error)
	}
	var listed struct {
		Tools []struct {
			Name string `json:"name"`
		} `json:"tools"`
	}
	if err := json.Unmarshal(response.Result, &listed); err != nil {
		t.Fatal(err)
	}
	var optional []string
	for _, tool := range listed.Tools {
		if strings.HasPrefix(tool.Name, "bootstrap") || tool.Name == "patterns" {
			continue
		}
		optional = append(optional, tool.Name)
	}
	want := []string{"conflicts", "daemons_list", "get_policy", "mcp_health", "mcp_list", "permissions_status", "ports_list", "ports_suggest", "scan", "version"}
	if !reflect.DeepEqual(optional, want) {
		t.Fatalf("optional tools = %v, want %v", optional, want)
	}

	writeJSON(t, cw, toolsCallRequest(3, "scan"))
	if response := readJSONResponse(t, cr); response.Error != nil {
		t.Fatalf("allowed call: %v", response.Error)
	}
	writeJSON(t, cw, toolsCallRequest(4, "scope_secret"))
	denied := readJSONResponse(t, cr)
	if denied.Error == nil || denied.Error.Code != -32601 {
		t.Fatalf("denied call = %+v, want JSON-RPC -32601", denied.Error)
	}
	data, err := os.ReadFile(callMarker)
	if err != nil {
		t.Fatal(err)
	}
	if got := string(data); got != "scan\n" {
		t.Fatalf("child calls = %q, want only allowed scan", got)
	}
}

func managedOptionalFake(t *testing.T, name, marker, tools string) *broker.ManagedServer {
	t.Helper()
	return broker.NewManagedServer(broker.ServerConfig{Name: name, BinaryPath: fakeBin, MaxRestarts: 0, Env: []string{"FAKEMCP_TOOLS=" + tools, "FAKEMCP_CALL_MARKER=" + marker}})
}
