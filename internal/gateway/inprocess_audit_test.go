package gateway

import (
	"context"
	"encoding/json"
	"log/slog"
	"os"
	"sync"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/audit"
	memoryconfig "github.com/danieljustus/symaira-brain/internal/memory/config"
	memdb "github.com/danieljustus/symaira-brain/internal/memory/db"
	memorymcp "github.com/danieljustus/symaira-brain/internal/memory/mcp"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
)

// auditCall is one recorded auditSink.Log invocation, captured by
// recordingAuditSink for assertions on server/tool/status.
type auditCall struct {
	server string
	tool   string
	status string
}

// recordingAuditSink implements auditSink and records every Log call's
// server/tool/status, so tests can assert on the shape of the emitted
// audit entry rather than just a call count.
type recordingAuditSink struct {
	mu    sync.Mutex
	calls []auditCall
}

func (r *recordingAuditSink) Log(server, tool string, _ json.RawMessage, _ time.Duration, status string, _ audit.Exposure, _ ...audit.Classification) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.calls = append(r.calls, auditCall{server: server, tool: tool, status: status})
}

func (r *recordingAuditSink) LogDegradation(string, string, string) {}

func (r *recordingAuditSink) Degraded() bool { return false }

func (r *recordingAuditSink) Close() error { return nil }

func (r *recordingAuditSink) snapshot() []auditCall {
	r.mu.Lock()
	defer r.mu.Unlock()
	return append([]auditCall(nil), r.calls...)
}

// helperMemoryServer builds a real, minimal in-process symmemory MCP
// server backed by a temp-dir sqlite database, mirroring the helper
// internal/memory/mcp's own tests use (helperServer in server_test.go).
// It lets the gateway test exercise the actual RegisterTools wiring
// end-to-end instead of a fake stand-in.
func helperMemoryServer(t *testing.T) *memorymcp.Server {
	t.Helper()

	tempDir, err := os.MkdirTemp("", "gateway-memory-audit-test-*")
	if err != nil {
		t.Fatalf("mkdir temp: %v", err)
	}
	t.Cleanup(func() { os.RemoveAll(tempDir) })

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	t.Cleanup(func() { os.Setenv("HOME", oldHome) })

	cfg := memoryconfig.Defaults()
	database, err := memdb.Open(cfg)
	if err != nil {
		t.Fatalf("open test database: %v", err)
	}
	t.Cleanup(func() { _ = database.Close() })

	jwtProvider, err := security.NewJWTProvider(cfg, nil)
	if err != nil {
		t.Fatalf("new JWT provider: %v", err)
	}

	return memorymcp.NewServer(database, jwtProvider, "test", cfg)
}

// TestServeIO_MemorySetProducesAuditEntry is the regression test for
// issue #422: the embedded in-process memory server's tools were
// registered directly on the mcpserver.Server with no audit wrapping, so
// memory_set/memory_promote/etc. calls never reached the JSONL audit log.
// This asserts a memory_set call through the real gateway wiring produces
// exactly the audit entry the catalog-routed path would have produced:
// server="memory", the original tool name, and status "ok".
func TestServeIO_MemorySetProducesAuditEntry(t *testing.T) {
	sink := &recordingAuditSink{}
	overrideAuditOpen(t, func(name string, cfg audit.Config) (auditSink, error) {
		return sink, nil
	})

	s := New(auditProfile(), nil, slog.Default(), nil, "dev")
	s.SetMemoryServer(helperMemoryServer(t))

	sr, sw, cr, cw := bidirectionalPipe(t)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	go func() { _ = s.ServeIO(ctx, sr, sw) }()

	writeJSON(t, cw, initializeRequest(1))
	if resp := readJSONResponse(t, cr); resp.Error != nil {
		t.Fatalf("initialize error: %v", resp.Error)
	}

	writeJSON(t, cw, map[string]any{
		"jsonrpc": "2.0",
		"id":      float64(2),
		"method":  "tools/call",
		"params": map[string]any{
			"name": "memory_set",
			"arguments": map[string]any{
				"content": "gateway audit regression test memory",
				"kind":    "reference",
			},
		},
	})
	resp := readJSONResponse(t, cr)
	if resp.Error != nil {
		t.Fatalf("tools/call(memory_set) error: %v", resp.Error)
	}
	var result struct {
		IsError bool `json:"isError"`
	}
	if err := json.Unmarshal(resp.Result, &result); err != nil {
		t.Fatalf("unmarshal tools/call result: %v", err)
	}
	if result.IsError {
		t.Fatalf("memory_set call reported isError: true, want a successful save")
	}

	var found *auditCall
	for _, c := range sink.snapshot() {
		if c.tool == "memory_set" {
			c := c
			found = &c
			break
		}
	}
	if found == nil {
		t.Fatal("no audit entry recorded for memory_set")
	}
	if found.server != "memory" {
		t.Errorf("memory_set audit entry server = %q, want %q", found.server, "memory")
	}
	if found.status != "ok" {
		t.Errorf("memory_set audit entry status = %q, want %q", found.status, "ok")
	}
}

// TestServeIO_SkillsToolProducesAuditEntry mirrors the memory_set
// regression above for the other gateway-owned in-process tool set:
// symskills tools are registered directly on the mcpserver.Server (see
// mcptools.Register in ServeIO), so this asserts a skills tool call
// produces an audit entry with server="skills" (issue #422).
func TestServeIO_SkillsToolProducesAuditEntry(t *testing.T) {
	sink := &recordingAuditSink{}
	overrideAuditOpen(t, func(name string, cfg audit.Config) (auditSink, error) {
		return sink, nil
	})

	s := New(auditProfile(), nil, slog.Default(), nil, "dev")

	sr, sw, cr, cw := bidirectionalPipe(t)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	go func() { _ = s.ServeIO(ctx, sr, sw) }()

	writeJSON(t, cw, initializeRequest(1))
	if resp := readJSONResponse(t, cr); resp.Error != nil {
		t.Fatalf("initialize error: %v", resp.Error)
	}

	writeJSON(t, cw, toolsCallRequest(2, "skills_targets_status"))
	resp := readJSONResponse(t, cr)
	if resp.Error != nil {
		t.Fatalf("tools/call(skills_targets_status) error: %v", resp.Error)
	}

	var found *auditCall
	for _, c := range sink.snapshot() {
		if c.tool == "skills_targets_status" {
			c := c
			found = &c
			break
		}
	}
	if found == nil {
		t.Fatal("no audit entry recorded for skills_targets_status")
	}
	if found.server != "skills" {
		t.Errorf("skills_targets_status audit entry server = %q, want %q", found.server, "skills")
	}
	if found.status != "ok" {
		t.Errorf("skills_targets_status audit entry status = %q, want %q", found.status, "ok")
	}
}
