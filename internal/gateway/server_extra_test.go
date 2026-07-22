package gateway

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"io"
	"log/slog"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/catalog"
	"github.com/danieljustus/symaira-brain/internal/profile"
)

// --- helpers ---

func newManagedFake(t *testing.T, name string, toolsJSON string) *broker.ManagedServer {
	t.Helper()
	ms := broker.NewManagedServer(broker.ServerConfig{
		Name:        name,
		BinaryPath:  fakeBin,
		MaxRestarts: 0,
		Env:         []string{"FAKEMCP_TOOLS=" + toolsJSON},
	})
	t.Cleanup(func() { ms.Shutdown() })

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	_, err := ms.ListTools(ctx)
	if err != nil {
		t.Fatalf("ListTools(%s): %v", name, err)
	}
	return ms
}

func testProfile() *profile.Profile {
	return &profile.Profile{
		Name: "test",
		Servers: profile.Servers{
			Vault:  profile.ServerConfig{Enabled: true, Mode: profile.VaultModeFull},
			Memory: profile.ServerConfig{Enabled: true, Mode: profile.MemoryModeReadWrite},
			Skills: profile.ServerConfig{Enabled: true},
		},
		Audit: profile.AuditConfig{Enabled: false},
	}
}

// --- New() tests ---

func TestNew_NilLogger(t *testing.T) {
	s := New(testProfile(), nil, nil)
	if s == nil {
		t.Fatal("New returned nil")
	}
	if s.logger == nil {
		t.Error("logger should default to slog.Default()")
	}
}

func TestNew_WithLogger(t *testing.T) {
	logger := slog.New(slog.NewTextHandler(bytes.NewBuffer(nil), nil))
	s := New(testProfile(), nil, logger)
	if s.logger != logger {
		t.Error("logger should be the one provided")
	}
}

// --- buildCatalog() tests ---

func TestBuildCatalog_MergesToolsFromMultipleServers(t *testing.T) {
	vault := newManagedFake(t, "vault",
		`[{"name":"get_entry","description":"fetch secret"},{"name":"health","description":"healthcheck"}]`)
	memory := newManagedFake(t, "memory",
		`[{"name":"memory_search","description":"search memories"},{"name":"entity_list","description":"list entities"}]`)

	servers := map[string]*broker.ManagedServer{
		"vault":  vault,
		"memory": memory,
	}

	s := New(testProfile(), servers, slog.Default())

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	if err := s.buildCatalog(ctx); err != nil {
		t.Fatalf("buildCatalog: %v", err)
	}
	if s.cat == nil {
		t.Fatal("catalog should be set after buildCatalog")
	}

	exposed := s.cat.Exposed()
	names := make(map[string]bool)
	for _, e := range exposed {
		names[e.Name] = true
	}

	for _, want := range []string{"vault_get_entry", "vault_health", "memory_search", "entity_list"} {
		if !names[want] {
			t.Errorf("missing exposed tool %q", want)
		}
	}
}

func TestBuildCatalog_SkipsDisabledServers(t *testing.T) {
	vault := newManagedFake(t, "vault",
		`[{"name":"get_entry","description":"fetch secret"}]`)

	p := testProfile()
	p.Servers.Memory = profile.ServerConfig{Enabled: false}

	servers := map[string]*broker.ManagedServer{
		"vault":  vault,
		"memory": newManagedFake(t, "memory", `[]`),
	}

	s := New(p, servers, slog.Default())

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	if err := s.buildCatalog(ctx); err != nil {
		t.Fatalf("buildCatalog: %v", err)
	}

	exposed := s.cat.Exposed()
	for _, e := range exposed {
		if e.Server == "memory" {
			t.Errorf("memory tool %q should not be exposed when memory is disabled", e.Name)
		}
	}
}

func TestBuildCatalog_PolicyFiltering(t *testing.T) {
	vault := newManagedFake(t, "vault",
		`[{"name":"get_entry","description":"fetch secret"},{"name":"health","description":"hc"},{"name":"request_credential","description":"req"}]`)

	p := testProfile()
	p.Servers.Vault.Mode = profile.VaultModeRequestOnly

	servers := map[string]*broker.ManagedServer{
		"vault": vault,
	}

	s := New(p, servers, slog.Default())

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	if err := s.buildCatalog(ctx); err != nil {
		t.Fatalf("buildCatalog: %v", err)
	}

	exposed := s.cat.Exposed()
	exposedNames := make(map[string]bool)
	for _, e := range exposed {
		exposedNames[e.Name] = true
	}

	if exposedNames["vault_get_entry"] {
		t.Error("vault_get_entry should be hidden in request_only mode")
	}
	if !exposedNames["vault_health"] {
		t.Error("vault_health should be exposed in request_only mode")
	}
	if !exposedNames["vault_request_credential"] {
		t.Error("vault_request_credential should be exposed in request_only mode")
	}
}

func TestBuildCatalog_EmptyServers(t *testing.T) {
	s := New(testProfile(), map[string]*broker.ManagedServer{}, slog.Default())

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	if err := s.buildCatalog(ctx); err != nil {
		t.Fatalf("buildCatalog with empty servers: %v", err)
	}

	exposed := s.cat.Exposed()
	if len(exposed) != 0 {
		t.Errorf("expected 0 exposed tools, got %d", len(exposed))
	}
}

// --- routeToolCall() tests ---

func TestRouteToolCall_Success(t *testing.T) {
	vault := newManagedFake(t, "vault",
		`[{"name":"get_entry","description":"fetch secret"}]`)

	s := New(testProfile(), map[string]*broker.ManagedServer{"vault": vault}, slog.Default())

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	if err := s.buildCatalog(ctx); err != nil {
		t.Fatalf("buildCatalog: %v", err)
	}

	var entry catalog.Entry
	for _, e := range s.cat.Exposed() {
		if e.Name == "vault_get_entry" {
			entry = e
			break
		}
	}
	if entry.Name == "" {
		t.Fatal("vault_get_entry not found in catalog")
	}

	result, err := s.routeToolCall(ctx, entry, json.RawMessage(`{"id":"test123"}`))
	if err != nil {
		t.Fatalf("routeToolCall: %v", err)
	}

	text, ok := result.(string)
	if !ok {
		t.Fatalf("result should be string, got %T", result)
	}
	if text == "" {
		t.Error("result text should not be empty")
	}
}

func TestRouteToolCall_MissingServer(t *testing.T) {
	s := New(testProfile(), map[string]*broker.ManagedServer{}, slog.Default())

	ctx := context.Background()
	entry := catalog.Entry{
		Server:       "nonexistent",
		OriginalName: "some_tool",
	}

	_, err := s.routeToolCall(ctx, entry, json.RawMessage(`{}`))
	if err == nil {
		t.Fatal("expected error for missing server")
	}
}

func TestRouteToolCall_ToolError(t *testing.T) {
	vault := newManagedFake(t, "vault",
		`[{"name":"get_entry","description":"always errors","behavior":"toolerror"}]`)

	s := New(testProfile(), map[string]*broker.ManagedServer{"vault": vault}, slog.Default())

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	if err := s.buildCatalog(ctx); err != nil {
		t.Fatalf("buildCatalog: %v", err)
	}

	var entry catalog.Entry
	for _, e := range s.cat.Exposed() {
		if e.Name == "vault_get_entry" {
			entry = e
			break
		}
	}
	if entry.Name == "" {
		t.Fatal("vault_get_entry not found in catalog")
	}

	_, err := s.routeToolCall(ctx, entry, json.RawMessage(`{}`))
	if err == nil {
		t.Fatal("expected error for tool error")
	}
}

func TestRouteToolCall_EmptyInput(t *testing.T) {
	vault := newManagedFake(t, "vault",
		`[{"name":"health","description":"healthcheck"}]`)

	s := New(testProfile(), map[string]*broker.ManagedServer{"vault": vault}, slog.Default())

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	if err := s.buildCatalog(ctx); err != nil {
		t.Fatalf("buildCatalog: %v", err)
	}

	var entry catalog.Entry
	for _, e := range s.cat.Exposed() {
		if e.Name == "vault_health" {
			entry = e
			break
		}
	}
	if entry.Name == "" {
		t.Fatal("vault_health not found in catalog")
	}

	result, err := s.routeToolCall(ctx, entry, nil)
	if err != nil {
		t.Fatalf("routeToolCall with nil input: %v", err)
	}
	if result == nil {
		t.Error("result should not be nil")
	}
}

// --- ServeIO() tests ---

// bidirectionalPipe returns reader/writer pairs for server and client sides.
// Server reads from sr, writes to sw. Client reads from cr, writes to cw.
func bidirectionalPipe(t *testing.T) (sr *io.PipeReader, sw *io.PipeWriter, cr *io.PipeReader, cw *io.PipeWriter) {
	t.Helper()
	// pipe1: client writes → server reads
	pipe1R, pipe1W := io.Pipe()
	// pipe2: server writes → client reads
	pipe2R, pipe2W := io.Pipe()
	return pipe1R, pipe2W, pipe2R, pipe1W
}

type rpcResponse struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      *int64          `json:"id,omitempty"`
	Result  json.RawMessage `json:"result,omitempty"`
	Error   *rpcError       `json:"error,omitempty"`
}

type rpcError struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
}

func writeJSON(t *testing.T, w io.Writer, v any) {
	t.Helper()
	data, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	data = append(data, '\n')
	if _, err := w.Write(data); err != nil {
		t.Fatalf("write: %v", err)
	}
}

func readJSONResponse(t *testing.T, r io.Reader) rpcResponse {
	t.Helper()
	scanner := bufio.NewScanner(r)
	scanner.Buffer(make([]byte, 0, 64*1024), 1<<20)
	if !scanner.Scan() {
		t.Fatal("no response received")
	}
	var resp rpcResponse
	if err := json.Unmarshal(scanner.Bytes(), &resp); err != nil {
		t.Fatalf("unmarshal response: %v\nraw: %s", err, scanner.Text())
	}
	return resp
}

func TestServeIO_InitializeAndListTools(t *testing.T) {
	vault := newManagedFake(t, "vault",
		`[{"name":"get_entry","description":"fetch secret"}]`)

	s := New(testProfile(), map[string]*broker.ManagedServer{"vault": vault}, slog.Default())

	sr, sw, cr, cw := bidirectionalPipe(t)

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	go func() {
		_ = s.ServeIO(ctx, sr, sw)
	}()

	writeJSON(t, cw, map[string]any{
		"jsonrpc": "2.0",
		"id":      float64(1),
		"method":  "initialize",
		"params": map[string]any{
			"protocolVersion": "2024-11-05",
			"capabilities":    map[string]any{},
			"clientInfo":      map[string]any{"name": "test", "version": "0.0.1"},
		},
	})
	resp := readJSONResponse(t, cr)
	if resp.Error != nil {
		t.Fatalf("initialize error: %v", resp.Error)
	}

	writeJSON(t, cw, map[string]any{
		"jsonrpc": "2.0",
		"id":      float64(2),
		"method":  "tools/list",
		"params":  map[string]any{},
	})
	listResp := readJSONResponse(t, cr)
	if listResp.Error != nil {
		t.Fatalf("tools/list error: %v", listResp.Error)
	}

	var result struct {
		Tools []struct {
			Name string `json:"name"`
		} `json:"tools"`
	}
	if err := json.Unmarshal(listResp.Result, &result); err != nil {
		t.Fatalf("unmarshal tools/list result: %v", err)
	}
	if len(result.Tools) == 0 {
		t.Error("expected at least one tool")
	}
	names := make(map[string]bool)
	for _, tool := range result.Tools {
		names[tool.Name] = true
	}
	if !names["vault_get_entry"] {
		t.Error("vault_get_entry should be in the tool list")
	}
}

func TestServeIO_ToolCall(t *testing.T) {
	vault := newManagedFake(t, "vault",
		`[{"name":"health","description":"healthcheck"}]`)

	s := New(testProfile(), map[string]*broker.ManagedServer{"vault": vault}, slog.Default())

	sr, sw, cr, cw := bidirectionalPipe(t)

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	go func() {
		_ = s.ServeIO(ctx, sr, sw)
	}()

	writeJSON(t, cw, map[string]any{
		"jsonrpc": "2.0", "id": float64(1), "method": "initialize",
		"params": map[string]any{
			"protocolVersion": "2024-11-05",
			"capabilities":    map[string]any{},
			"clientInfo":      map[string]any{"name": "test", "version": "0.0.1"},
		},
	})
	readJSONResponse(t, cr)

	writeJSON(t, cw, map[string]any{
		"jsonrpc": "2.0", "id": float64(2), "method": "tools/call",
		"params": map[string]any{
			"name":      "vault_health",
			"arguments": map[string]any{},
		},
	})
	resp := readJSONResponse(t, cr)
	if resp.Error != nil {
		t.Fatalf("tools/call error: %v", resp.Error)
	}

	var toolResult struct {
		Content []struct {
			Text string `json:"text"`
		} `json:"content"`
	}
	if err := json.Unmarshal(resp.Result, &toolResult); err != nil {
		t.Fatalf("unmarshal tools/call result: %v", err)
	}
	if len(toolResult.Content) == 0 {
		t.Fatal("expected content in tool call result")
	}
}

func TestServeIO_ClosedReaderReturnsNil(t *testing.T) {
	s := New(testProfile(), nil, slog.Default())

	sr, sw, cr, cw := bidirectionalPipe(t)
	cw.Close()
	cr.Close()
	sw.Close()

	_ = sr

	err := s.ServeIO(context.Background(), sr, sw)
	if err != nil {
		t.Errorf("ServeIO on closed reader should return nil, got: %v", err)
	}
}
