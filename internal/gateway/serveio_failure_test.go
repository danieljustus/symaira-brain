package gateway

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"log/slog"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/audit"
	"github.com/danieljustus/symaira-brain/internal/broker"
	"github.com/danieljustus/symaira-brain/internal/config"
	"github.com/danieljustus/symaira-brain/internal/profile"
)

// --- helpers ---

// recordingHandler captures slog messages for assertions.
type recordingHandler struct {
	mu       sync.Mutex
	messages []string
}

func (h *recordingHandler) Enabled(context.Context, slog.Level) bool { return true }

func (h *recordingHandler) Handle(_ context.Context, r slog.Record) error {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.messages = append(h.messages, r.Message)
	return nil
}

func (h *recordingHandler) WithAttrs([]slog.Attr) slog.Handler { return h }
func (h *recordingHandler) WithGroup(string) slog.Handler      { return h }

func (h *recordingHandler) count(msg string) int {
	h.mu.Lock()
	defer h.mu.Unlock()
	n := 0
	for _, m := range h.messages {
		if m == msg {
			n++
		}
	}
	return n
}

// fakeAuditSink implements auditSink with a fixed Degraded verdict and a
// call counter, so tests can drive the degraded-warning path.
type fakeAuditSink struct {
	degraded bool
	logCalls atomic.Int64
}

func (f *fakeAuditSink) Log(string, string, json.RawMessage, time.Duration, string, ...audit.Classification) {
	f.logCalls.Add(1)
}

func (f *fakeAuditSink) LogDegradation(string, string, string) {}

func (f *fakeAuditSink) Degraded() bool { return f.degraded }

func (f *fakeAuditSink) Close() error { return nil }

// overrideAuditOpen swaps the auditOpen seam for the duration of the test.
func overrideAuditOpen(t *testing.T, fn func(name string, cfg audit.Config) (auditSink, error)) {
	t.Helper()
	orig := auditOpen
	auditOpen = fn
	t.Cleanup(func() { auditOpen = orig })
}

func auditProfile() *profile.Profile {
	p := testProfile()
	p.Audit.Enabled = true
	return p
}

func initializeRequest(id float64) map[string]any {
	return map[string]any{
		"jsonrpc": "2.0",
		"id":      id,
		"method":  "initialize",
		"params": map[string]any{
			"protocolVersion": "2024-11-05",
			"capabilities":    map[string]any{},
			"clientInfo":      map[string]any{"name": "test", "version": "0.0.1"},
		},
	}
}

func toolsCallRequest(id float64, name string) map[string]any {
	return map[string]any{
		"jsonrpc": "2.0",
		"id":      id,
		"method":  "tools/call",
		"params": map[string]any{
			"name":      name,
			"arguments": map[string]any{},
		},
	}
}

// --- ServeIO startup failure modes ---

func TestServeIO_CatalogBuildFailure(t *testing.T) {
	// The memory child exposes a tool that collides with the vault
	// namespace after prefixing: "vault_get_entry" vs vault's "get_entry".
	// catalog.Build treats this as a hard startup error.
	vault := newManagedFake(t, "vault",
		`[{"name":"get_entry","description":"fetch secret"}]`)
	memory := newManagedFake(t, "memory",
		`[{"name":"vault_get_entry","description":"collides with the vault namespace"}]`)

	s := New(testProfile(), map[string]*broker.ManagedServer{
		"vault":  vault,
		"memory": memory,
	}, slog.Default(), nil, "dev")

	sr, sw, cr, _ := bidirectionalPipe(t)

	err := s.ServeIO(context.Background(), sr, sw)
	if err == nil {
		t.Fatal("ServeIO should fail when the merged catalog cannot be built")
	}
	msg := err.Error()
	for _, want := range []string{
		"gateway: build catalog",
		"vault_get_entry",
		"collision",
	} {
		if !strings.Contains(msg, want) {
			t.Errorf("ServeIO error %q should contain %q", msg, want)
		}
	}

	// The harness gets no partial JSON-RPC handshake: ServeIO returns
	// before the MCP loop starts, so the client sees EOF, not a response.
	sw.Close()
	scanner := bufio.NewScanner(cr)
	if scanner.Scan() {
		t.Errorf("client should receive no JSON-RPC response on catalog build failure, got: %s", scanner.Text())
	}
}

func TestServeIO_AuditOpenFailure(t *testing.T) {
	overrideAuditOpen(t, func(name string, cfg audit.Config) (auditSink, error) {
		return nil, errors.New("audit: open test.jsonl: permission denied")
	})

	rec := &recordingHandler{}
	vault := newManagedFake(t, "vault",
		`[{"name":"health","description":"healthcheck"}]`)

	// cfg != nil exercises the global-audit merge branch in ServeIO.
	cfg := &config.Config{Audit: config.AuditConfig{Enabled: true, Verbose: true}}
	s := New(auditProfile(), map[string]*broker.ManagedServer{"vault": vault}, slog.New(rec), cfg, "dev")

	sr, sw, cr, cw := bidirectionalPipe(t)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	go func() { _ = s.ServeIO(ctx, sr, sw) }()

	// Audit open failure is degraded, not fatal: the handshake still
	// completes with a well-formed JSON-RPC response.
	writeJSON(t, cw, initializeRequest(1))
	resp := readJSONResponse(t, cr)
	if resp.Error != nil {
		t.Fatalf("initialize returned an error after audit open failure: %v", resp.Error)
	}
	if resp.JSONRPC != "2.0" {
		t.Errorf("jsonrpc = %q, want %q", resp.JSONRPC, "2.0")
	}
	if resp.ID == nil || *resp.ID != 1 {
		t.Errorf("response id = %v, want 1", resp.ID)
	}

	// A tool call still round-trips.
	writeJSON(t, cw, toolsCallRequest(2, "vault_health"))
	toolResp := readJSONResponse(t, cr)
	if toolResp.Error != nil {
		t.Fatalf("tools/call returned an error after audit open failure: %v", toolResp.Error)
	}
	if toolResp.ID == nil || *toolResp.ID != 2 {
		t.Errorf("tools/call response id = %v, want 2", toolResp.ID)
	}
	var toolResult struct {
		Content []struct {
			Text string `json:"text"`
		} `json:"content"`
	}
	if err := json.Unmarshal(toolResp.Result, &toolResult); err != nil {
		t.Fatalf("unmarshal tools/call result: %v", err)
	}
	if len(toolResult.Content) == 0 {
		t.Error("expected content in tools/call result")
	}

	// The failure is surfaced as a warning on the gateway log.
	if got := rec.count("failed to open audit log"); got != 1 {
		t.Errorf("expected exactly one \"failed to open audit log\" warning, got %d", got)
	}
}

func TestServeIO_AuditDegradedWarning(t *testing.T) {
	sink := &fakeAuditSink{degraded: true}
	overrideAuditOpen(t, func(name string, cfg audit.Config) (auditSink, error) {
		return sink, nil
	})

	rec := &recordingHandler{}
	vault := newManagedFake(t, "vault",
		`[{"name":"health","description":"healthcheck"}]`)
	// memory_set is in the read_write preset, so it is exposed; the
	// toolerror behavior makes it fail when called.
	memory := newManagedFake(t, "memory",
		`[{"name":"memory_set","description":"always errors","behavior":"toolerror"}]`)

	s := New(auditProfile(), map[string]*broker.ManagedServer{
		"vault":  vault,
		"memory": memory,
	}, slog.New(rec), nil, "dev")

	sr, sw, cr, cw := bidirectionalPipe(t)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	go func() { _ = s.ServeIO(ctx, sr, sw) }()

	writeJSON(t, cw, initializeRequest(1))
	if resp := readJSONResponse(t, cr); resp.Error != nil {
		t.Fatalf("initialize error: %v", resp.Error)
	}

	const warn = "audit log degraded; some entries may not be persisted"

	// Two tool calls through the degraded logger: the warning is guarded
	// by sync.Once, so it must be emitted exactly once.
	for id := float64(2); id <= 3; id++ {
		writeJSON(t, cw, toolsCallRequest(id, "vault_health"))
		resp := readJSONResponse(t, cr)
		if resp.Error != nil {
			t.Fatalf("tools/call(%v) error: %v", id, resp.Error)
		}
	}

	// A failing tool call logs status "error" through the same degraded
	// sink: the MCP shape is a result-level isError envelope, and the
	// degraded warning still fires only once.
	writeJSON(t, cw, toolsCallRequest(4, "memory_set"))
	errResp := readJSONResponse(t, cr)
	if errResp.Error != nil {
		t.Fatalf("tools/call error response: %v", errResp.Error)
	}
	var errResult struct {
		IsError bool `json:"isError"`
	}
	if err := json.Unmarshal(errResp.Result, &errResult); err != nil {
		t.Fatalf("unmarshal tools/call error result: %v", err)
	}
	if !errResult.IsError {
		t.Error("failing tool call should be reported with isError: true")
	}

	if got := sink.logCalls.Load(); got != 3 {
		t.Errorf("audit sink Log calls = %d, want 3", got)
	}
	if got := rec.count(warn); got != 1 {
		t.Errorf("degraded warning emitted %d times, want exactly 1; messages: %v", got, rec.messages)
	}
}
