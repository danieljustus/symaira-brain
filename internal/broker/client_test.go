package broker

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"testing"
	"time"
)

// testPeer simulates a child MCP server's stdio side of the connection
// using an in-process io.Pipe, so protocol-level behavior (request/response
// correlation, timeouts, RPC errors, closed connections) can be unit
// tested without spawning a real subprocess. Integration tests against a
// real spawned child live in client_integration_test.go (see #12's fake
// MCP binary).
type testPeer struct {
	t *testing.T
	r *bufio.Reader
	w io.WriteCloser
}

func newTestClient(t *testing.T) (*Client, *testPeer) {
	t.Helper()
	inR, inW := io.Pipe()   // client writes requests to inW; peer reads from inR
	outR, outW := io.Pipe() // peer writes responses to outW; client reads from outR

	c := newClient(inW, outR)
	t.Cleanup(func() {
		_ = c.Close()
		_ = inR.Close()
		_ = outW.Close()
	})

	return c, &testPeer{t: t, r: bufio.NewReader(inR), w: outW}
}

// next reads and decodes the next line the client wrote (a request or a
// notification).
func (p *testPeer) next() map[string]any {
	p.t.Helper()
	line, err := p.r.ReadString('\n')
	if err != nil {
		p.t.Fatalf("testPeer: read line: %v", err)
	}
	var msg map[string]any
	if err := json.Unmarshal([]byte(line), &msg); err != nil {
		p.t.Fatalf("testPeer: decode line %q: %v", line, err)
	}
	return msg
}

func (p *testPeer) respond(id float64, result any) {
	p.t.Helper()
	data, err := json.Marshal(map[string]any{
		"jsonrpc": "2.0",
		"id":      id,
		"result":  result,
	})
	if err != nil {
		p.t.Fatalf("testPeer: marshal response: %v", err)
	}
	if _, err := p.w.Write(append(data, '\n')); err != nil {
		p.t.Fatalf("testPeer: write response: %v", err)
	}
}

func (p *testPeer) respondError(id float64, code int, message string) {
	p.t.Helper()
	data, err := json.Marshal(map[string]any{
		"jsonrpc": "2.0",
		"id":      id,
		"error":   map[string]any{"code": code, "message": message},
	})
	if err != nil {
		p.t.Fatalf("testPeer: marshal error response: %v", err)
	}
	if _, err := p.w.Write(append(data, '\n')); err != nil {
		p.t.Fatalf("testPeer: write error response: %v", err)
	}
}

func TestClient_Initialize(t *testing.T) {
	c, peer := newTestClient(t)

	done := make(chan struct{})
	var result *InitializeResult
	var callErr error
	go func() {
		result, callErr = c.Initialize(context.Background())
		close(done)
	}()

	req := peer.next()
	if req["method"] != "initialize" {
		t.Fatalf("method = %v, want initialize", req["method"])
	}
	params, _ := req["params"].(map[string]any)
	if params["protocolVersion"] != protocolVersion {
		t.Errorf("protocolVersion = %v, want %v", params["protocolVersion"], protocolVersion)
	}
	clientInfoMap, _ := params["clientInfo"].(map[string]any)
	if clientInfoMap["name"] != clientName {
		t.Errorf("clientInfo.name = %v, want %v", clientInfoMap["name"], clientName)
	}

	peer.respond(req["id"].(float64), map[string]any{
		"protocolVersion": "2024-11-05",
		"serverInfo":      map[string]string{"name": "fakemcp", "version": "1.2.3"},
		"instructions":    "hello",
	})

	// The client must also send the notifications/initialized notification
	// (no "id" field at all) after a successful initialize.
	notif := peer.next()
	if notif["method"] != "notifications/initialized" {
		t.Fatalf("method = %v, want notifications/initialized", notif["method"])
	}
	if _, hasID := notif["id"]; hasID {
		t.Errorf("notifications/initialized must not carry an id field, got %v", notif["id"])
	}

	<-done
	if callErr != nil {
		t.Fatalf("Initialize() error = %v", callErr)
	}
	if result.ServerInfo.Name != "fakemcp" || result.ServerInfo.Version != "1.2.3" {
		t.Errorf("ServerInfo = %+v", result.ServerInfo)
	}
	if result.Instructions != "hello" {
		t.Errorf("Instructions = %q, want %q", result.Instructions, "hello")
	}
}

func TestClient_ListTools(t *testing.T) {
	c, peer := newTestClient(t)

	done := make(chan struct{})
	var tools []Tool
	var callErr error
	go func() {
		tools, callErr = c.ListTools(context.Background())
		close(done)
	}()

	req := peer.next()
	if req["method"] != "tools/list" {
		t.Fatalf("method = %v, want tools/list", req["method"])
	}
	peer.respond(req["id"].(float64), map[string]any{
		"tools": []map[string]any{
			{"name": "get_entry", "description": "fetch an entry", "inputSchema": map[string]any{"type": "object"}},
			{"name": "health", "description": "health check"},
		},
	})

	<-done
	if callErr != nil {
		t.Fatalf("ListTools() error = %v", callErr)
	}
	if len(tools) != 2 || tools[0].Name != "get_entry" || tools[1].Name != "health" {
		t.Fatalf("tools = %+v", tools)
	}
}

func TestClient_CallTool_Success(t *testing.T) {
	c, peer := newTestClient(t)

	done := make(chan struct{})
	var result *CallToolResult
	var callErr error
	go func() {
		result, callErr = c.CallTool(context.Background(), "health", json.RawMessage(`{}`))
		close(done)
	}()

	req := peer.next()
	if req["method"] != "tools/call" {
		t.Fatalf("method = %v, want tools/call", req["method"])
	}
	params, _ := req["params"].(map[string]any)
	if params["name"] != "health" {
		t.Errorf("params.name = %v, want health", params["name"])
	}
	peer.respond(req["id"].(float64), map[string]any{
		"content": []map[string]any{{"type": "text", "text": "ok"}},
		"isError": false,
	})

	<-done
	if callErr != nil {
		t.Fatalf("CallTool() error = %v", callErr)
	}
	if result.IsError {
		t.Errorf("IsError = true, want false")
	}
	if len(result.Content) != 1 || result.Content[0].Text != "ok" {
		t.Fatalf("Content = %+v", result.Content)
	}
}

func TestClient_CallTool_ToolLevelError(t *testing.T) {
	// A tool-level failure is reported via isError:true in a *successful*
	// JSON-RPC response — it must not surface as a Go error.
	c, peer := newTestClient(t)

	done := make(chan struct{})
	var result *CallToolResult
	var callErr error
	go func() {
		result, callErr = c.CallTool(context.Background(), "get_entry", json.RawMessage(`{"id":"missing"}`))
		close(done)
	}()

	req := peer.next()
	peer.respond(req["id"].(float64), map[string]any{
		"content": []map[string]any{{"type": "text", "text": "entry not found"}},
		"isError": true,
	})

	<-done
	if callErr != nil {
		t.Fatalf("CallTool() error = %v, want nil (tool errors are not Go errors)", callErr)
	}
	if !result.IsError {
		t.Errorf("IsError = false, want true")
	}
}

func TestClient_CallTool_RPCError(t *testing.T) {
	c, peer := newTestClient(t)

	done := make(chan struct{})
	var callErr error
	go func() {
		_, callErr = c.CallTool(context.Background(), "unknown_tool", nil)
		close(done)
	}()

	req := peer.next()
	peer.respondError(req["id"].(float64), -32601, "Unknown tool: unknown_tool")

	<-done
	var rpcErr *RPCError
	if !errors.As(callErr, &rpcErr) {
		t.Fatalf("error = %v (%T), want *RPCError", callErr, callErr)
	}
	if rpcErr.Code != -32601 {
		t.Errorf("Code = %d, want -32601", rpcErr.Code)
	}
}

func TestClient_CallTool_Timeout(t *testing.T) {
	c, peer := newTestClient(t)

	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()

	done := make(chan struct{})
	var callErr error
	go func() {
		_, callErr = c.CallTool(ctx, "slow", nil)
		close(done)
	}()

	// Consume the request so the write doesn't block, but never respond.
	peer.next()

	<-done
	var timeoutErr *TimeoutError
	if !errors.As(callErr, &timeoutErr) {
		t.Fatalf("error = %v (%T), want *TimeoutError", callErr, callErr)
	}
}

func TestClient_CallTool_ContextCanceled(t *testing.T) {
	c, peer := newTestClient(t)

	ctx, cancel := context.WithCancel(context.Background())

	done := make(chan struct{})
	var callErr error
	go func() {
		_, callErr = c.CallTool(ctx, "slow", nil)
		close(done)
	}()

	peer.next()
	cancel()

	<-done
	if !errors.Is(callErr, context.Canceled) {
		t.Fatalf("error = %v, want wrapped context.Canceled", callErr)
	}
	var timeoutErr *TimeoutError
	if errors.As(callErr, &timeoutErr) {
		t.Errorf("got *TimeoutError for an explicit cancellation, want a plain cancellation error")
	}
}

func TestClient_ClosedChild(t *testing.T) {
	c, peer := newTestClient(t)

	done := make(chan struct{})
	var callErr error
	go func() {
		_, callErr = c.CallTool(context.Background(), "health", nil)
		close(done)
	}()

	peer.next()
	// Simulate the child crashing: close its write end without responding.
	_ = peer.w.Close()

	<-done
	var closedErr *ClosedError
	if !errors.As(callErr, &closedErr) {
		t.Fatalf("error = %v (%T), want *ClosedError", callErr, callErr)
	}

	select {
	case <-c.Done():
	case <-time.After(time.Second):
		t.Fatal("Done() did not close after the child's stdout closed")
	}
}

func TestClient_CallAfterClose_ReturnsClosedError(t *testing.T) {
	c, peer := newTestClient(t)
	_ = peer.w.Close()

	select {
	case <-c.Done():
	case <-time.After(time.Second):
		t.Fatal("Done() did not close")
	}

	_, err := c.CallTool(context.Background(), "health", nil)
	var closedErr *ClosedError
	if !errors.As(err, &closedErr) {
		t.Fatalf("error = %v (%T), want *ClosedError", err, err)
	}
}

func TestClient_NoBackingProcess(t *testing.T) {
	c, _ := newTestClient(t)
	if c.Pid() != 0 {
		t.Errorf("Pid() = %d, want 0", c.Pid())
	}
	if c.Process() != nil {
		t.Errorf("Process() = %v, want nil", c.Process())
	}
	if c.Path() != "" {
		t.Errorf("Path() = %q, want empty", c.Path())
	}
}

func TestDiscover_Override(t *testing.T) {
	// The running test binary itself always exists and is a fine stand-in
	// for "a file that exists" without depending on any Symaira binary
	// being on PATH.
	self, err := os.Executable()
	if err != nil {
		t.Fatalf("resolve test binary: %v", err)
	}
	got, err := Discover("whatever", self)
	if err != nil {
		t.Fatalf("Discover() error = %v", err)
	}
	if got != self {
		t.Errorf("Discover() = %q, want %q", got, self)
	}
}

func TestDiscover_OverrideMissing(t *testing.T) {
	_, err := Discover("whatever", "/nonexistent/path/to/binary")
	if err == nil {
		t.Fatal("Discover() error = nil, want error for a missing override path")
	}
}

func TestDiscover_PathLookup(t *testing.T) {
	_, err := Discover("definitely-not-a-real-symaira-binary-xyz", "")
	if err == nil {
		t.Fatal("Discover() error = nil, want error for a binary absent from PATH")
	}
}
