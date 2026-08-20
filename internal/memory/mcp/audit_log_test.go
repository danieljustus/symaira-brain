package mcp

import (
	"context"
	"encoding/json"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
	"github.com/danieljustus/symaira-brain/internal/memory/db"
	"github.com/danieljustus/symaira-brain/internal/memory/security"
)

// TestServiceLogAuditArgumentMapping guards the MemoryService.LogAudit
// argument contract: (action, memoryID, scope, session, actor, detail) must
// land on the matching audit_log columns. The wrapper previously named its
// parameters entityID/memoryID/diff, which invited callers to shift every
// value one column over (entityID into memory_id, memoryID into scope, diff
// into session).
func TestServiceLogAuditArgumentMapping(t *testing.T) {
	s := helperServer(t)
	s.service.db.SetAuditLogEnabled(true)
	t.Cleanup(func() { s.service.db.SetAuditLogEnabled(true) })

	if err := s.service.LogAudit("set", "mem-arg-1", "global", "sess-arg", "actor-arg", "detail-arg"); err != nil {
		t.Fatalf("LogAudit failed: %v", err)
	}

	events, err := s.service.db.GetAuditLogs("set", 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 {
		t.Fatalf("expected 1 audit event, got %d", len(events))
	}
	e := events[0]
	if e.Action != "set" {
		t.Errorf("action = %q, want %q", e.Action, "set")
	}
	if e.MemoryID != "mem-arg-1" {
		t.Errorf("memory_id = %q, want %q", e.MemoryID, "mem-arg-1")
	}
	if e.Scope != "global" {
		t.Errorf("scope = %q, want %q", e.Scope, "global")
	}
	if e.Session != "sess-arg" {
		t.Errorf("session = %q, want %q", e.Session, "sess-arg")
	}
	if e.Actor != "actor-arg" {
		t.Errorf("actor = %q, want %q", e.Actor, "actor-arg")
	}
	if e.Detail != "detail-arg" {
		t.Errorf("detail = %q, want %q", e.Detail, "detail-arg")
	}
}

// TestMemorySetWritesAuditEvents verifies the MCP memory_set write path
// records a "set" audit row with the correct memory_id, scope, session and
// actor, and that the PII redaction performed during the write produces a
// redaction audit event carrying pattern labels — never the matched values.
func TestMemorySetWritesAuditEvents(t *testing.T) {
	s := helperServer(t)
	s.service.db.SetAuditLogEnabled(true)
	t.Cleanup(func() { s.service.db.SetAuditLogEnabled(true) })

	text, err := s.handleMemorySet(context.Background(), json.RawMessage(
		`{"content":"Contact alice@example.com for the vault token","kind":"user","scope":"global","session_id":"sess-mcp-1"}`,
	))
	if err != nil {
		t.Fatalf("handleMemorySet failed: %v", err)
	}
	id := memoryIDFromSetText(t, text.(string))

	// The set event: actor falls back to "mcp" without an initialize handshake.
	setEvents, err := s.service.db.GetAuditLogs("set", 10)
	if err != nil {
		t.Fatal(err)
	}
	var setEvent *db.AuditEvent
	for _, e := range setEvents {
		if e.MemoryID == id {
			setEvent = e
			break
		}
	}
	if setEvent == nil {
		t.Fatalf("expected a set audit event for %s, got %+v", id, setEvents)
	}
	if setEvent.Scope != "global" {
		t.Errorf("set event scope = %q, want %q", setEvent.Scope, "global")
	}
	if setEvent.Session != "sess-mcp-1" {
		t.Errorf("set event session = %q, want %q", setEvent.Session, "sess-mcp-1")
	}
	if setEvent.Actor != fallbackMCPSource {
		t.Errorf("set event actor = %q, want %q", setEvent.Actor, fallbackMCPSource)
	}
	if setEvent.Detail != "" {
		t.Errorf("set event detail must stay empty, got %q", setEvent.Detail)
	}

	// The redaction event: pattern labels only.
	redEvents, err := s.service.db.GetAuditLogs(db.EventRedaction, 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(redEvents) == 0 {
		t.Fatal("expected a redaction audit event from the memory_set write")
	}
	if !strings.Contains(redEvents[0].Detail, "email") {
		t.Errorf("redaction detail should carry the pattern label 'email', got %q", redEvents[0].Detail)
	}
	if strings.Contains(redEvents[0].Detail, "alice@example.com") {
		t.Error("raw matched value leaked into redaction audit detail")
	}
}

// TestMemorySetAuditDisabled verifies that audit_log_enabled=false suppresses
// audit writes on the MCP memory_set path without failing the write itself.
func TestMemorySetAuditDisabled(t *testing.T) {
	cfg := config.Defaults()
	cfg.Retention.AuditLogEnabled = false
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := db.Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		_ = database.Close()
		database.SetAuditLogEnabled(true)
	})
	jwtProvider, err := security.NewJWTProvider(config.Defaults(), nil)
	if err != nil {
		t.Fatal(err)
	}
	s := NewServer(database, jwtProvider, "test", cfg)

	text, err := s.handleMemorySet(context.Background(), json.RawMessage(`{"content":"secret note","kind":"user","scope":"global"}`))
	if err != nil {
		t.Fatalf("handleMemorySet must not fail when audit logging is disabled: %v", err)
	}
	if text == "" {
		t.Fatal("expected a success response")
	}

	events, err := database.GetAuditLogs("", 100)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 0 {
		t.Fatalf("expected no audit rows with audit_log_enabled=false, got %d", len(events))
	}
}
