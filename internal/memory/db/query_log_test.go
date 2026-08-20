package db

import (
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/danieljustus/symaira-brain/internal/memory/config"
)

// openTestDBWithConfig opens a test database with a custom configuration,
// mirroring openTestDB (token_test.go).
func openTestDBWithConfig(t *testing.T, cfg *config.Config) *DB {
	t.Helper()
	tempDir, err := os.MkdirTemp("", "symmemory-querylog-test-*")
	if err != nil {
		t.Fatalf("failed to create temp dir: %v", err)
	}
	t.Cleanup(func() { os.RemoveAll(tempDir) })

	oldHome := os.Getenv("HOME")
	os.Setenv("HOME", tempDir)
	t.Cleanup(func() { os.Setenv("HOME", oldHome) })

	database, err := Open(cfg)
	if err != nil {
		t.Fatalf("failed to open database: %v", err)
	}
	t.Cleanup(func() { _ = database.Close() })
	return database
}

// insertQueryLogRows seeds query_log rows directly with controllable
// created_at values (increasing by one minute per row, starting at base).
// The rows use the pre-attribution column set, so identity columns stay NULL.
func insertQueryLogRows(t *testing.T, database *DB, n int, base time.Time, prefix string) {
	t.Helper()
	tx, err := database.conn.Begin()
	if err != nil {
		t.Fatalf("failed to begin tx: %v", err)
	}
	for i := 0; i < n; i++ {
		id := fmt.Sprintf("%s-%04d", prefix, i)
		if _, err := tx.Exec(
			`INSERT INTO query_log (id, tool, query_text, params, duration_ms, created_at)
			 VALUES (?, ?, ?, ?, ?, ?)`,
			id, "memory_search", nullStr("query "+id), nullStr(`{"k":"v"}`), int64(i), base.Add(time.Duration(i)*time.Minute),
		); err != nil {
			_ = tx.Rollback()
			t.Fatalf("failed to insert query log row %s: %v", id, err)
		}
	}
	if err := tx.Commit(); err != nil {
		t.Fatalf("failed to commit tx: %v", err)
	}
}

func TestLogQueryInsertsEntryWithToolAttribution(t *testing.T) {
	database := openTestDB(t)

	if _, err := database.LogQuery("mcp", "", "", "memory_search", "what is symmemory", `{"limit":5}`, 42); err != nil {
		t.Fatalf("LogQuery: %v", err)
	}
	if _, err := database.LogQuery("mcp", "", "", "memory_get", "", "", 7); err != nil {
		t.Fatalf("LogQuery: %v", err)
	}

	entries, err := database.GetQueryLogEntries(10)
	if err != nil {
		t.Fatalf("GetQueryLogEntries: %v", err)
	}
	if len(entries) != 2 {
		t.Fatalf("expected 2 entries, got %d", len(entries))
	}

	byTool := map[string]*QueryLogEntry{}
	for _, e := range entries {
		byTool[e.Tool] = e
	}

	search := byTool["memory_search"]
	if search == nil {
		t.Fatalf("expected memory_search entry, got %v", byTool)
	}
	if search.QueryText != "what is symmemory" || search.Params != `{"limit":5}` || search.DurationMs != 42 {
		t.Errorf("memory_search entry mismatch: %+v", search)
	}
	if search.ID == "" || search.CreatedAt.IsZero() {
		t.Errorf("expected non-empty id and created_at, got %+v", search)
	}

	// Empty query text and params are stored as NULL and read back as "".
	get := byTool["memory_get"]
	if get == nil {
		t.Fatalf("expected memory_get entry, got %v", byTool)
	}
	if get.QueryText != "" || get.Params != "" {
		t.Errorf("expected empty query/params for NULL columns, got %+v", get)
	}
}

func TestPruneQueryLogRetentionCap(t *testing.T) {
	database := openTestDB(t)
	base := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)

	// 1005 rows: 5 more than the 1000-entry cap.
	insertQueryLogRows(t, database, maxQueryLogEntries+5, base, "prune")

	if err := database.pruneQueryLog(); err != nil {
		t.Fatalf("pruneQueryLog: %v", err)
	}

	var count int
	if err := database.conn.QueryRow(`SELECT COUNT(*) FROM query_log`).Scan(&count); err != nil {
		t.Fatal(err)
	}
	if count != maxQueryLogEntries {
		t.Fatalf("expected %d rows after prune, got %d", maxQueryLogEntries, count)
	}

	// The 5 oldest entries (ids prune-0000..prune-0004) must be gone.
	for _, id := range []string{"prune-0000", "prune-0002", "prune-0004"} {
		var exists int
		if err := database.conn.QueryRow(`SELECT COUNT(*) FROM query_log WHERE id = ?`, id).Scan(&exists); err != nil {
			t.Fatal(err)
		}
		if exists != 0 {
			t.Errorf("expected pruned id %s to be gone", id)
		}
	}
	// The first surviving entry must be prune-0005.
	var firstID string
	if err := database.conn.QueryRow(`SELECT id FROM query_log ORDER BY created_at ASC LIMIT 1`).Scan(&firstID); err != nil {
		t.Fatal(err)
	}
	if firstID != "prune-0005" {
		t.Errorf("expected prune-0005 as oldest survivor, got %s", firstID)
	}

	// Pruning at or below the cap is a no-op.
	if err := database.pruneQueryLog(); err != nil {
		t.Fatalf("pruneQueryLog (no-op): %v", err)
	}
	if err := database.conn.QueryRow(`SELECT COUNT(*) FROM query_log`).Scan(&count); err != nil {
		t.Fatal(err)
	}
	if count != maxQueryLogEntries {
		t.Errorf("expected count unchanged after no-op prune, got %d", count)
	}
}

func TestGetQueryLogEntriesLimitAndOrdering(t *testing.T) {
	database := openTestDB(t)
	base := time.Date(2026, 2, 1, 0, 0, 0, 0, time.UTC)
	insertQueryLogRows(t, database, 5, base, "ord")

	// Most recent first, newest id = ord-0004.
	entries, err := database.GetQueryLogEntries(3)
	if err != nil {
		t.Fatalf("GetQueryLogEntries(3): %v", err)
	}
	if len(entries) != 3 {
		t.Fatalf("expected 3 entries, got %d", len(entries))
	}
	for i, e := range entries {
		wantID := fmt.Sprintf("ord-%04d", 4-i)
		if e.ID != wantID {
			t.Errorf("entry %d: expected %s, got %s", i, wantID, e.ID)
		}
		if e.CreatedAt.IsZero() {
			t.Errorf("entry %d: zero created_at", i)
		}
	}

	// Non-positive limit defaults to 50 → all 5.
	for _, limit := range []int{0, -1} {
		all, err := database.GetQueryLogEntries(limit)
		if err != nil {
			t.Fatalf("GetQueryLogEntries(%d): %v", limit, err)
		}
		if len(all) != 5 {
			t.Errorf("GetQueryLogEntries(%d): expected 5, got %d", limit, len(all))
		}
	}

	// Oversized limit is capped at maxQueryLogEntries.
	all, err := database.GetQueryLogEntries(maxQueryLogEntries + 100)
	if err != nil {
		t.Fatalf("GetQueryLogEntries(cap): %v", err)
	}
	if len(all) != 5 {
		t.Errorf("capped limit: expected 5, got %d", len(all))
	}
}

func TestGetQueryLogSummaryAggregation(t *testing.T) {
	database := openTestDB(t)
	base := time.Date(2026, 3, 1, 0, 0, 0, 0, time.UTC)

	tx, err := database.conn.Begin()
	if err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 5; i++ {
		tool := "memory_search"
		if i >= 3 {
			tool = "memory_get"
		}
		if _, err := tx.Exec(
			`INSERT INTO query_log (id, tool, query_text, params, duration_ms, created_at)
			 VALUES (?, ?, ?, ?, ?, ?)`,
			fmt.Sprintf("sum-%04d", i), tool, nullStr("q"), nullStr("{}"), int64(i), base.Add(time.Duration(i)*time.Minute),
		); err != nil {
			_ = tx.Rollback()
			t.Fatal(err)
		}
	}
	if err := tx.Commit(); err != nil {
		t.Fatal(err)
	}

	summary, err := database.GetQueryLogSummary(10, "")
	if err != nil {
		t.Fatalf("GetQueryLogSummary: %v", err)
	}
	if summary.TotalQueries != 5 {
		t.Errorf("TotalQueries = %d, want 5", summary.TotalQueries)
	}
	if summary.ToolBreakdown["memory_search"] != 3 || summary.ToolBreakdown["memory_get"] != 2 {
		t.Errorf("ToolBreakdown = %v, want {memory_search:3 memory_get:2}", summary.ToolBreakdown)
	}
	if len(summary.RecentEntries) != 5 {
		t.Errorf("RecentEntries len = %d, want 5", len(summary.RecentEntries))
	}
	if summary.RecentEntries[0].ID != "sum-0004" {
		t.Errorf("newest recent entry = %s, want sum-0004", summary.RecentEntries[0].ID)
	}

	// Limit applies to recent entries only.
	limited, err := database.GetQueryLogSummary(2, "")
	if err != nil {
		t.Fatalf("GetQueryLogSummary(2): %v", err)
	}
	if limited.TotalQueries != 5 {
		t.Errorf("limited TotalQueries = %d, want 5", limited.TotalQueries)
	}
	if len(limited.RecentEntries) != 2 {
		t.Errorf("limited RecentEntries len = %d, want 2", len(limited.RecentEntries))
	}
}

func TestQueryLogPruneCount(t *testing.T) {
	database := openTestDB(t)
	base := time.Date(2026, 4, 1, 0, 0, 0, 0, time.UTC)

	// Empty table.
	n, err := database.QueryLogPruneCount()
	if err != nil {
		t.Fatalf("QueryLogPruneCount: %v", err)
	}
	if n != 0 {
		t.Errorf("empty table prune count = %d, want 0", n)
	}

	// Exactly at cap.
	insertQueryLogRows(t, database, maxQueryLogEntries, base, "pc-cap")
	n, err = database.QueryLogPruneCount()
	if err != nil {
		t.Fatalf("QueryLogPruneCount: %v", err)
	}
	if n != 0 {
		t.Errorf("at-cap prune count = %d, want 0", n)
	}

	// Over the cap by 5.
	insertQueryLogRows(t, database, 5, base.Add(time.Duration(maxQueryLogEntries)*time.Minute), "pc-over")
	n, err = database.QueryLogPruneCount()
	if err != nil {
		t.Fatalf("QueryLogPruneCount: %v", err)
	}
	if n != 5 {
		t.Errorf("over-cap prune count = %d, want 5", n)
	}
}

func TestLogQueryRecordsActorScopeSession(t *testing.T) {
	database := openTestDB(t)

	if _, err := database.LogQuery("claude/1.0", "project", "sess-abc", "memory_search", "what is symmemory", "{}", 42); err != nil {
		t.Fatalf("LogQuery: %v", err)
	}
	if _, err := database.LogQuery("", "", "", "memory_get", "", "", 7); err != nil {
		t.Fatalf("LogQuery: %v", err)
	}

	entries, err := database.GetQueryLogEntries(10)
	if err != nil {
		t.Fatalf("GetQueryLogEntries: %v", err)
	}
	if len(entries) != 2 {
		t.Fatalf("expected 2 entries, got %d", len(entries))
	}

	byTool := map[string]*QueryLogEntry{}
	for _, e := range entries {
		byTool[e.Tool] = e
	}

	search := byTool["memory_search"]
	if search == nil {
		t.Fatalf("expected memory_search entry, got %v", byTool)
	}
	if search.Actor != "claude/1.0" || search.Scope != "project" || search.Session != "sess-abc" {
		t.Errorf("expected attributed entry, got %+v", search)
	}

	// Empty identity fields are stored as NULL and read back as "".
	get := byTool["memory_get"]
	if get == nil {
		t.Fatalf("expected memory_get entry, got %v", byTool)
	}
	if get.Actor != "" || get.Scope != "" || get.Session != "" {
		t.Errorf("expected empty identity for NULL columns, got %+v", get)
	}
}

func TestGetQueryLogSummaryActorBreakdownAndFilter(t *testing.T) {
	database := openTestDB(t)

	// 2 searches by alice, 1 get by bob, 1 legacy row with NULL actor.
	if _, err := database.LogQuery("alice", "project", "s1", "memory_search", "q1", "{}", 1); err != nil {
		t.Fatalf("LogQuery: %v", err)
	}
	if _, err := database.LogQuery("alice", "project", "s2", "memory_search", "q2", "{}", 2); err != nil {
		t.Fatalf("LogQuery: %v", err)
	}
	if _, err := database.LogQuery("bob", "global", "s3", "memory_get", "", "", 3); err != nil {
		t.Fatalf("LogQuery: %v", err)
	}
	if _, err := database.conn.Exec(
		`INSERT INTO query_log (id, tool, query_text, duration_ms, created_at) VALUES (?, 'memory_list', 'legacy', 1, ?)`,
		"legacy-1", time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC),
	); err != nil {
		t.Fatalf("insert legacy row: %v", err)
	}

	summary, err := database.GetQueryLogSummary(10, "")
	if err != nil {
		t.Fatalf("GetQueryLogSummary: %v", err)
	}
	if summary.TotalQueries != 4 {
		t.Errorf("TotalQueries = %d, want 4", summary.TotalQueries)
	}
	if summary.ActorBreakdown["alice"] != 2 || summary.ActorBreakdown["bob"] != 1 || summary.ActorBreakdown["(unknown)"] != 1 {
		t.Errorf("ActorBreakdown = %v, want {alice:2 bob:1 (unknown):1}", summary.ActorBreakdown)
	}

	// Actor filter narrows total, tool breakdown and recent entries.
	filtered, err := database.GetQueryLogSummary(10, "alice")
	if err != nil {
		t.Fatalf("GetQueryLogSummary(10, alice): %v", err)
	}
	if filtered.TotalQueries != 2 {
		t.Errorf("filtered TotalQueries = %d, want 2", filtered.TotalQueries)
	}
	if filtered.ToolBreakdown["memory_search"] != 2 || len(filtered.ToolBreakdown) != 1 {
		t.Errorf("filtered ToolBreakdown = %v, want {memory_search:2}", filtered.ToolBreakdown)
	}
	if len(filtered.RecentEntries) != 2 {
		t.Fatalf("filtered RecentEntries len = %d, want 2", len(filtered.RecentEntries))
	}
	for _, e := range filtered.RecentEntries {
		if e.Actor != "alice" {
			t.Errorf("filtered recent entry actor = %q, want alice", e.Actor)
		}
	}
}

func TestPruneQueryLogConfiguredCap(t *testing.T) {
	cfg := config.Defaults()
	cfg.QueryLog.MaxEntries = 5
	database := openTestDBWithConfig(t, cfg)

	for i := 0; i < 10; i++ {
		if _, err := database.LogQuery("alice", "", "", "memory_search", fmt.Sprintf("q%d", i), "", int64(i)); err != nil {
			t.Fatalf("LogQuery %d: %v", i, err)
		}
	}

	var count int
	if err := database.conn.QueryRow(`SELECT COUNT(*) FROM query_log`).Scan(&count); err != nil {
		t.Fatal(err)
	}
	if count != 5 {
		t.Fatalf("expected 5 rows after prune at configured cap, got %d", count)
	}

	// The 5 oldest entries (q0..q4) must be gone.
	var firstQ string
	if err := database.conn.QueryRow(`SELECT query_text FROM query_log ORDER BY created_at ASC LIMIT 1`).Scan(&firstQ); err != nil {
		t.Fatal(err)
	}
	if firstQ != "q5" {
		t.Errorf("expected q5 as oldest survivor, got %s", firstQ)
	}

	n, err := database.QueryLogPruneCount()
	if err != nil {
		t.Fatalf("QueryLogPruneCount: %v", err)
	}
	if n != 0 {
		t.Errorf("QueryLogPruneCount = %d, want 0", n)
	}
}

func TestPruneQueryLogMaxAge(t *testing.T) {
	cfg := config.Defaults()
	cfg.QueryLog.MaxAge = "1h"
	database := openTestDBWithConfig(t, cfg)

	// One row older than the max age, one fresh row written via LogQuery
	// (which prunes on insert).
	old := time.Now().UTC().Add(-2 * time.Hour)
	if _, err := database.conn.Exec(
		`INSERT INTO query_log (id, tool, query_text, duration_ms, created_at) VALUES ('old-1', 'memory_search', 'stale', 1, ?)`,
		old,
	); err != nil {
		t.Fatalf("insert old row: %v", err)
	}
	if _, err := database.LogQuery("alice", "", "", "memory_search", "fresh", "", 1); err != nil {
		t.Fatalf("LogQuery: %v", err)
	}

	entries, err := database.GetQueryLogEntries(10)
	if err != nil {
		t.Fatalf("GetQueryLogEntries: %v", err)
	}
	if len(entries) != 1 {
		t.Fatalf("expected 1 entry after age prune, got %d", len(entries))
	}
	if entries[0].QueryText != "fresh" {
		t.Errorf("expected fresh entry to survive, got %+v", entries[0])
	}
}

func TestQueryLogMigrationAdditiveUpgrade(t *testing.T) {
	database := openTestDB(t)

	// Rebuild query_log in its pre-029 (original 027) shape.
	for _, stmt := range []string{
		`DROP INDEX IF EXISTS idx_query_log_actor`,
		`DROP INDEX IF EXISTS idx_query_log_tool`,
		`DROP INDEX IF EXISTS idx_query_log_created_at`,
		`DROP TABLE IF EXISTS query_log`,
		`CREATE TABLE query_log (
			id          TEXT PRIMARY KEY,
			tool        TEXT NOT NULL,
			query_text  TEXT,
			params      TEXT,
			duration_ms INTEGER NOT NULL DEFAULT 0,
			created_at  DATETIME NOT NULL DEFAULT (datetime('now'))
		)`,
	} {
		if _, err := database.conn.Exec(stmt); err != nil {
			t.Fatalf("rebuild schema (%q): %v", stmt, err)
		}
	}

	// Seed a legacy row exactly as the old schema would store it.
	legacyTime := time.Date(2026, 1, 2, 3, 4, 5, 0, time.UTC)
	if _, err := database.conn.Exec(
		`INSERT INTO query_log (id, tool, query_text, params, duration_ms, created_at) VALUES (?, ?, ?, ?, ?, ?)`,
		"legacy-1", "memory_search", "old query", `{"k":"v"}`, 12, legacyTime,
	); err != nil {
		t.Fatal(err)
	}

	// Apply the actual 029 migration file on top of the old schema.
	migration, err := migrationFS.ReadFile("migrations/029_query_log_attribution.sql")
	if err != nil {
		t.Fatalf("read migration: %v", err)
	}
	if _, err := database.conn.Exec(string(migration)); err != nil {
		t.Fatalf("apply 029 migration: %v", err)
	}

	// The legacy row survives with NULL identity columns read back as "".
	entries, err := database.GetQueryLogEntries(10)
	if err != nil {
		t.Fatalf("GetQueryLogEntries: %v", err)
	}
	if len(entries) != 1 {
		t.Fatalf("expected legacy row to survive, got %d entries", len(entries))
	}
	e := entries[0]
	if e.ID != "legacy-1" || e.QueryText != "old query" || e.DurationMs != 12 {
		t.Errorf("legacy row content mismatch: %+v", e)
	}
	if e.Actor != "" || e.Scope != "" || e.Session != "" {
		t.Errorf("expected NULL identity columns on legacy row, got %+v", e)
	}

	// New writes with identity land in the upgraded schema.
	if _, err := database.LogQuery("alice", "project", "s1", "memory_search", "new query", "", 3); err != nil {
		t.Fatalf("LogQuery after upgrade: %v", err)
	}
	entries, err = database.GetQueryLogEntries(10)
	if err != nil {
		t.Fatalf("GetQueryLogEntries: %v", err)
	}
	if len(entries) != 2 {
		t.Fatalf("expected 2 entries after upgrade write, got %d", len(entries))
	}
}
