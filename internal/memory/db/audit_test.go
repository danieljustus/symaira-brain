package db

import (
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

func TestAuditLogRoundTrip(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	if err := database.LogAudit("set", "mem-1", "global", "session-1", "user-1", `{"key":"val"}`); err != nil {
		t.Fatal(err)
	}

	events, err := database.GetAuditLogs("set", 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 {
		t.Fatalf("expected 1 event, got %d", len(events))
	}
	if events[0].Action != "set" {
		t.Errorf("expected action 'set', got %q", events[0].Action)
	}
	if events[0].MemoryID != "mem-1" {
		t.Errorf("expected memory_id 'mem-1', got %q", events[0].MemoryID)
	}
}

func TestRedactionAuditEvent(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	if err := database.LogRedactionAudit("mem-1", "global", "session-1", "user-1", []string{"email", "api_key"}); err != nil {
		t.Fatal(err)
	}

	events, err := database.GetAuditLogs(EventRedaction, 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 {
		t.Fatalf("expected 1 event, got %d", len(events))
	}
	if events[0].Action != EventRedaction {
		t.Errorf("expected action %q, got %q", EventRedaction, events[0].Action)
	}
	if events[0].MemoryID != "mem-1" {
		t.Errorf("expected memory_id 'mem-1', got %q", events[0].MemoryID)
	}
	if events[0].Detail != "email,api_key" {
		t.Errorf("expected detail 'email,api_key', got %q", events[0].Detail)
	}
	// Verify raw values never appear in audit detail.
	if strings.Contains(events[0].Detail, "alice@example.com") {
		t.Error("raw matched value leaked into audit detail")
	}
}

func TestPurgeExpiredMemories(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()

	old := &Memory{
		ID:        "old-session",
		Content:   "old fact",
		Scope:     "session",
		Metadata:  map[string]string{},
		CreatedAt: time.Now().Add(-48 * time.Hour),
	}
	if err := database.SaveMemory(old); err != nil {
		t.Fatal(err)
	}

	new_ := &Memory{
		ID:       "new-session",
		Content:  "new fact",
		Scope:    "session",
		Metadata: map[string]string{},
	}
	if err := database.SaveMemory(new_); err != nil {
		t.Fatal(err)
	}

	n, err := database.PurgeExpiredMemories(24*time.Hour, "cli:tester")
	if err != nil {
		t.Fatal(err)
	}
	if n != 1 {
		t.Errorf("expected 1 purged, got %d", n)
	}

	remaining, _ := database.GetMemory("old-session")
	if remaining != nil {
		t.Errorf("expected old-session to be purged")
	}
	still, _ := database.GetMemory("new-session")
	if still == nil {
		t.Errorf("expected new-session to remain")
	}

	// The purge must leave an audit trace with the requesting actor.
	events, err := database.GetAuditLogs("purge", 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 {
		t.Fatalf("expected 1 purge audit event, got %d", len(events))
	}
	if events[0].Scope != "session" {
		t.Errorf("expected purge event scope 'session', got %q", events[0].Scope)
	}
	if events[0].Actor != "cli:tester" {
		t.Errorf("expected purge event actor 'cli:tester', got %q", events[0].Actor)
	}
}

// TestMutationAuditEvents verifies that every memory mutation path at the
// storage layer — set, update, delete, purge — records an audit row with the
// correct action, memory_id, scope, session and actor.
func TestMutationAuditEvents(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()
	database.SetAuditLogEnabled(true)
	t.Cleanup(func() { database.SetAuditLogEnabled(true) })

	// set (insert via SaveMemory)
	created := &Memory{
		ID:             "mem-set-1",
		Content:        "fact one",
		Scope:          "global",
		Metadata:       map[string]string{},
		CreatedBy:      "cli:tester",
		CreatedSession: "sess-1",
	}
	if err := database.SaveMemory(created); err != nil {
		t.Fatal(err)
	}
	setEvents, err := database.GetAuditLogs("set", 10)
	if err != nil {
		t.Fatal(err)
	}
	e := findAuditEvent(t, setEvents, "mem-set-1")
	if e == nil {
		t.Fatal("expected a set audit event for mem-set-1")
	}
	if e.Scope != "global" || e.Session != "sess-1" || e.Actor != "cli:tester" {
		t.Errorf("set event columns wrong: scope=%q session=%q actor=%q", e.Scope, e.Session, e.Actor)
	}
	if e.Detail != "" {
		t.Errorf("set event detail must stay empty, got %q", e.Detail)
	}

	// update (newer upsert)
	updated := &Memory{
		ID:             "mem-set-1",
		Content:        "fact one v2",
		Scope:          "global",
		Metadata:       map[string]string{},
		UpdatedAt:      time.Now().UTC().Add(time.Hour),
		UpdatedBy:      "cli:updater",
		UpdatedSession: "sess-2",
	}
	ok, err := database.UpsertMemoryIfNewer(updated)
	if err != nil {
		t.Fatal(err)
	}
	if !ok {
		t.Fatal("expected upsert to apply the newer row")
	}
	updateEvents, err := database.GetAuditLogs("update", 10)
	if err != nil {
		t.Fatal(err)
	}
	e = findAuditEvent(t, updateEvents, "mem-set-1")
	if e == nil {
		t.Fatal("expected an update audit event for mem-set-1")
	}
	if e.Scope != "global" || e.Session != "sess-2" || e.Actor != "cli:updater" {
		t.Errorf("update event columns wrong: scope=%q session=%q actor=%q", e.Scope, e.Session, e.Actor)
	}

	// set (insert path of upsert)
	upserted := &Memory{
		ID:             "mem-ins-1",
		Content:        "fact two",
		Scope:          "project",
		Metadata:       map[string]string{},
		CreatedAt:      time.Now().UTC(),
		UpdatedAt:      time.Now().UTC(),
		CreatedBy:      "cli:creator",
		CreatedSession: "sess-3",
	}
	ok, err = database.UpsertMemoryIfNewer(upserted)
	if err != nil {
		t.Fatal(err)
	}
	if !ok {
		t.Fatal("expected upsert insert to report a change")
	}
	setEvents, err = database.GetAuditLogs("set", 10)
	if err != nil {
		t.Fatal(err)
	}
	if e := findAuditEvent(t, setEvents, "mem-ins-1"); e == nil {
		t.Fatal("expected a set audit event for the upsert insert mem-ins-1")
	}

	// delete
	if err := database.DeleteMemory("mem-set-1"); err != nil {
		t.Fatal(err)
	}
	deleteEvents, err := database.GetAuditLogs("delete", 10)
	if err != nil {
		t.Fatal(err)
	}
	e = findAuditEvent(t, deleteEvents, "mem-set-1")
	if e == nil {
		t.Fatal("expected a delete audit event for mem-set-1")
	}
	if e.Scope != "global" || e.Session != "sess-1" || e.Actor != "cli:tester" {
		t.Errorf("delete event columns wrong: scope=%q session=%q actor=%q", e.Scope, e.Session, e.Actor)
	}

	// purge by id
	purged, err := database.PurgeByID("mem-ins-1", "cli:purger")
	if err != nil {
		t.Fatal(err)
	}
	if !purged {
		t.Fatal("expected purge by id to remove the memory")
	}
	purgeEvents, err := database.GetAuditLogs("purge", 10)
	if err != nil {
		t.Fatal(err)
	}
	e = findAuditEvent(t, purgeEvents, "mem-ins-1")
	if e == nil {
		t.Fatal("expected a purge audit event for mem-ins-1")
	}
	if e.Scope != "project" || e.Session != "sess-3" || e.Actor != "cli:purger" {
		t.Errorf("purge-by-id event columns wrong: scope=%q session=%q actor=%q", e.Scope, e.Session, e.Actor)
	}

	// purge by scope (bulk, memory_id empty, count in detail)
	scoped := &Memory{ID: "mem-scope-1", Content: "scoped fact", Scope: "user", Metadata: map[string]string{}, CreatedBy: "cli:creator"}
	if err := database.SaveMemory(scoped); err != nil {
		t.Fatal(err)
	}
	n, err := database.PurgeByScope("user", "cli:purger")
	if err != nil {
		t.Fatal(err)
	}
	if n != 1 {
		t.Fatalf("expected 1 scoped purge, got %d", n)
	}
	purgeEvents, err = database.GetAuditLogs("purge", 10)
	if err != nil {
		t.Fatal(err)
	}
	var bulk *AuditEvent
	for _, ev := range purgeEvents {
		if ev.MemoryID == "" && ev.Scope == "user" {
			bulk = ev
			break
		}
	}
	if bulk == nil {
		t.Fatal("expected a bulk purge audit event for scope 'user'")
	}
	if bulk.Actor != "cli:purger" || bulk.Detail != "purged=1" {
		t.Errorf("bulk purge event wrong: actor=%q detail=%q", bulk.Actor, bulk.Detail)
	}

	// purge expired session memories
	old := &Memory{ID: "mem-old-1", Content: "old session fact", Scope: "session", Metadata: map[string]string{}, CreatedAt: time.Now().UTC().Add(-48 * time.Hour)}
	if err := database.SaveMemory(old); err != nil {
		t.Fatal(err)
	}
	n, err = database.PurgeExpiredMemories(24*time.Hour, "cli:purger")
	if err != nil {
		t.Fatal(err)
	}
	if n != 1 {
		t.Fatalf("expected 1 expired purge, got %d", n)
	}
	purgeEvents, err = database.GetAuditLogs("purge", 10)
	if err != nil {
		t.Fatal(err)
	}
	var expired *AuditEvent
	for _, ev := range purgeEvents {
		if ev.MemoryID == "" && ev.Scope == "session" {
			expired = ev
			break
		}
	}
	if expired == nil {
		t.Fatal("expected an expired-memories purge audit event")
	}
	if expired.Actor != "cli:purger" || expired.Detail != "purged=1" {
		t.Errorf("expired purge event wrong: actor=%q detail=%q", expired.Actor, expired.Detail)
	}
}

// TestAuditLogDisabled verifies that audit_log_enabled=false suppresses every
// audit write (direct, redaction and mutation paths) without failing the
// underlying operations.
func TestAuditLogDisabled(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()
	database.SetAuditLogEnabled(false)
	t.Cleanup(func() { database.SetAuditLogEnabled(true) })

	if err := database.LogAudit("set", "mem-x", "global", "", "a", "detail"); err != nil {
		t.Fatalf("LogAudit must not fail when disabled: %v", err)
	}
	if err := database.LogRedactionAudit("mem-x", "global", "", "a", []string{"email"}); err != nil {
		t.Fatalf("LogRedactionAudit must not fail when disabled: %v", err)
	}

	m := &Memory{ID: "mem-disabled-1", Content: "fact", Scope: "global", Metadata: map[string]string{}, CreatedBy: "cli:tester"}
	if err := database.SaveMemory(m); err != nil {
		t.Fatalf("SaveMemory must not fail when audit is disabled: %v", err)
	}
	if err := database.DeleteMemory(m.ID); err != nil {
		t.Fatalf("DeleteMemory must not fail when audit is disabled: %v", err)
	}
	again := &Memory{ID: "mem-disabled-2", Content: "fact", Scope: "user", Metadata: map[string]string{}}
	if err := database.SaveMemory(again); err != nil {
		t.Fatal(err)
	}
	if n, err := database.PurgeByScope("user", "cli:purger"); err != nil || n != 1 {
		t.Fatalf("PurgeByScope must not fail when audit is disabled (n=%d err=%v)", n, err)
	}
	if n, err := database.PurgeExpiredAuditLogs(24 * time.Hour); err != nil || n != 0 {
		t.Fatalf("PurgeExpiredAuditLogs must not fail when disabled (n=%d err=%v)", n, err)
	}

	events, err := database.GetAuditLogs("", 100)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 0 {
		t.Fatalf("expected no audit rows when disabled, got %d", len(events))
	}
}

// TestPurgeExpiredAuditLogsRetention verifies that audit rows older than the
// configured retention window are pruned while fresh rows survive.
func TestPurgeExpiredAuditLogsRetention(t *testing.T) {
	cfg := config.Defaults()
	cfg.Database.Path = t.TempDir() + "/test.db"
	database, err := Open(cfg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = database.Close() }()
	database.SetAuditLogEnabled(true)
	t.Cleanup(func() { database.SetAuditLogEnabled(true) })

	if err := database.LogAudit("set", "mem-fresh", "global", "", "a", ""); err != nil {
		t.Fatal(err)
	}
	if err := database.LogAudit("set", "mem-stale", "global", "", "a", ""); err != nil {
		t.Fatal(err)
	}
	if _, err := database.conn.Exec("UPDATE audit_log SET created_at = ? WHERE memory_id = 'mem-stale'", time.Now().UTC().Add(-48*time.Hour)); err != nil {
		t.Fatal(err)
	}

	n, err := database.PurgeExpiredAuditLogs(24 * time.Hour)
	if err != nil {
		t.Fatal(err)
	}
	if n != 1 {
		t.Fatalf("expected 1 pruned audit row, got %d", n)
	}

	events, err := database.GetAuditLogs("set", 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 || events[0].MemoryID != "mem-fresh" {
		t.Fatalf("expected only the fresh audit row to remain, got %+v", events)
	}
}

// findAuditEvent returns the event for the given memory id, or nil.
func findAuditEvent(t *testing.T, events []*AuditEvent, memoryID string) *AuditEvent {
	t.Helper()
	for _, e := range events {
		if e.MemoryID == memoryID {
			return e
		}
	}
	return nil
}
