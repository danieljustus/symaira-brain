package db

import (
	"database/sql"
	"time"

	"github.com/google/uuid"
)

// maxQueryLogEntries is the default row cap for the query log, applied when
// no [query_log] retention policy is configured. It preserves the historical
// behavior (issue #457).
const maxQueryLogEntries = 1000

// QueryLogEntry represents a single query log row.
type QueryLogEntry struct {
	ID         string    `json:"id"`
	Actor      string    `json:"actor,omitempty"`   // client identity that issued the query
	Scope      string    `json:"scope,omitempty"`   // scope the query ran in
	Session    string    `json:"session,omitempty"` // session id carried by the request
	Tool       string    `json:"tool"`
	QueryText  string    `json:"query_text,omitempty"`
	Params     string    `json:"params,omitempty"`
	DurationMs int64     `json:"duration_ms"`
	CreatedAt  time.Time `json:"created_at"`
}

// QueryLogResult records one memory that a retrieval returned, linked to
// the query-log row that caused it (issue #460). It stores a reference —
// the memory id and its rank/score — never a copy of the memory content.
type QueryLogResult struct {
	QueryID  string  `json:"query_id"`
	MemoryID string  `json:"memory_id"`
	Rank     int     `json:"rank"`
	Score    float64 `json:"score"`
}

// QueryLogSummary holds aggregated stats for the query-log summary CLI.
type QueryLogSummary struct {
	TotalQueries   int              `json:"total_queries"`
	ToolBreakdown  map[string]int   `json:"tool_breakdown"`
	ActorBreakdown map[string]int   `json:"actor_breakdown"`
	RecentEntries  []*QueryLogEntry `json:"recent_entries"`
	PrunedCount    int              `json:"pruned_count,omitempty"`
}

// LogQuery inserts a new query log entry attributed to the given actor
// (the client identity that issued the query), scope and session, then
// prunes entries that exceed the configured retention policy. It returns
// the inserted query id so callers can attach result-set records.
func (db *DB) LogQuery(actor, scope, session, tool, queryText, params string, durationMs int64) (string, error) {
	id := uuid.New().String()
	now := time.Now().UTC()

	if _, err := db.conn.Exec(
		`INSERT INTO query_log (id, actor, scope, session, tool, query_text, params, duration_ms, created_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		id, nullStr(actor), nullStr(scope), nullStr(session), tool, nullStr(queryText), nullStr(params), durationMs, now,
	); err != nil {
		return "", err
	}

	if err := db.pruneQueryLog(); err != nil {
		return "", err
	}
	return id, nil
}

// RecordQueryResults links a logged query to the memories its retrieval
// returned, one row per memory with rank and score. It is a no-op when
// result recording is disabled or the result list is empty. Recording
// errors are returned to the caller, which treats them as non-fatal
// telemetry failures.
func (db *DB) RecordQueryResults(queryID string, results []QueryResultRef) error {
	if !db.queryLogRecordResults.Load() {
		return nil
	}
	if len(results) == 0 {
		return nil
	}
	for _, r := range results {
		if _, err := db.conn.Exec(
			`INSERT OR IGNORE INTO query_log_results (query_id, memory_id, rank, score) VALUES (?, ?, ?, ?)`,
			queryID, r.MemoryID, r.Rank, r.Score,
		); err != nil {
			return err
		}
	}
	return nil
}

// GetQueryLogResults resolves a logged query to the memories it returned,
// ordered by retrieval rank.
func (db *DB) GetQueryLogResults(queryID string) ([]*QueryLogResult, error) {
	rows, err := db.conn.Query(
		`SELECT query_id, memory_id, rank, score FROM query_log_results WHERE query_id = ? ORDER BY rank ASC`,
		queryID,
	)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var out []*QueryLogResult
	for rows.Next() {
		var r QueryLogResult
		if err := rows.Scan(&r.QueryID, &r.MemoryID, &r.Rank, &r.Score); err != nil {
			return nil, err
		}
		out = append(out, &r)
	}
	if out == nil {
		out = []*QueryLogResult{}
	}
	return out, nil
}

// QueryResultRef is one returned memory reference for RecordQueryResults.
type QueryResultRef struct {
	MemoryID string
	Rank     int
	Score    float64
}

// pruneQueryLog removes entries that exceed the configured retention policy:
// the row cap (oldest first) and, when configured, entries older than the
// max age. The row cap defaults to maxQueryLogEntries.
func (db *DB) pruneQueryLog() error {
	limit := db.effectiveQueryLogMaxEntries()

	var count int
	if err := db.conn.QueryRow("SELECT COUNT(*) FROM query_log").Scan(&count); err != nil {
		return err
	}
	if count > limit {
		excess := count - limit
		if _, err := db.conn.Exec(
			`DELETE FROM query_log WHERE id IN (
				SELECT id FROM query_log ORDER BY created_at ASC LIMIT ?
			)`, excess,
		); err != nil {
			return err
		}
	}

	if db.queryLogMaxAge > 0 {
		cutoff := time.Now().UTC().Add(-db.queryLogMaxAge)
		if _, err := db.conn.Exec(`DELETE FROM query_log WHERE created_at < ?`, cutoff); err != nil {
			return err
		}
	}
	return nil
}

// effectiveQueryLogMaxEntries returns the configured query log row cap,
// falling back to maxQueryLogEntries when unset or invalid.
func (db *DB) effectiveQueryLogMaxEntries() int {
	if db.queryLogMaxEntries > 0 {
		return db.queryLogMaxEntries
	}
	return maxQueryLogEntries
}

// GetQueryLogEntries returns the most recent query log entries.
func (db *DB) GetQueryLogEntries(limit int) ([]*QueryLogEntry, error) {
	return db.getQueryLogEntries(limit, "")
}

// getQueryLogEntries returns the most recent query log entries, optionally
// narrowed to a single actor (empty actor means no filter).
func (db *DB) getQueryLogEntries(limit int, actor string) ([]*QueryLogEntry, error) {
	if limit <= 0 {
		limit = 50
	}
	if limit > db.effectiveQueryLogMaxEntries() {
		limit = db.effectiveQueryLogMaxEntries()
	}

	query := `SELECT id, actor, scope, session, tool, query_text, params, duration_ms, created_at
	 FROM query_log`
	var args []any
	if actor != "" {
		query += ` WHERE actor = ?`
		args = append(args, actor)
	}
	query += ` ORDER BY created_at DESC LIMIT ?`
	args = append(args, limit)

	rows, err := db.conn.Query(query, args...)
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var entries []*QueryLogEntry
	for rows.Next() {
		var e QueryLogEntry
		var a, sc, sess, qt, p sql.NullString
		if err := rows.Scan(&e.ID, &a, &sc, &sess, &e.Tool, &qt, &p, &e.DurationMs, &e.CreatedAt); err != nil {
			return nil, err
		}
		if a.Valid {
			e.Actor = a.String
		}
		if sc.Valid {
			e.Scope = sc.String
		}
		if sess.Valid {
			e.Session = sess.String
		}
		if qt.Valid {
			e.QueryText = qt.String
		}
		if p.Valid {
			e.Params = p.String
		}
		entries = append(entries, &e)
	}
	return entries, nil
}

// GetQueryLogSummary aggregates query log data for the CLI summary. When
// actor is non-empty the summary (total, tool and actor breakdowns, recent
// entries) is narrowed to rows recorded for that actor.
func (db *DB) GetQueryLogSummary(limit int, actor string) (*QueryLogSummary, error) {
	where := ""
	var args []any
	if actor != "" {
		where = " WHERE actor = ?"
		args = append(args, actor)
	}

	var total int
	if err := db.conn.QueryRow("SELECT COUNT(*) FROM query_log"+where, args...).Scan(&total); err != nil {
		return nil, err
	}

	summary := &QueryLogSummary{
		TotalQueries:   total,
		ToolBreakdown:  make(map[string]int),
		ActorBreakdown: make(map[string]int),
	}

	// Tool breakdown
	rows, err := db.conn.Query("SELECT tool, COUNT(*) as cnt FROM query_log"+where+" GROUP BY tool ORDER BY cnt DESC", args...)
	if err != nil {
		return nil, err
	}
	for rows.Next() {
		var tool string
		var cnt int
		if err := rows.Scan(&tool, &cnt); err != nil {
			_ = rows.Close()
			return nil, err
		}
		summary.ToolBreakdown[tool] = cnt
	}
	_ = rows.Close()
	if err := rows.Err(); err != nil {
		return nil, err
	}

	// Actor breakdown. Rows written before the attribution migration have a
	// NULL actor; they surface under an explicit "(unknown)" bucket.
	rows, err = db.conn.Query("SELECT actor, COUNT(*) as cnt FROM query_log"+where+" GROUP BY actor ORDER BY cnt DESC", args...)
	if err != nil {
		return nil, err
	}
	for rows.Next() {
		var a sql.NullString
		var cnt int
		if err := rows.Scan(&a, &cnt); err != nil {
			_ = rows.Close()
			return nil, err
		}
		key := "(unknown)"
		if a.Valid && a.String != "" {
			key = a.String
		}
		summary.ActorBreakdown[key] = cnt
	}
	_ = rows.Close()
	if err := rows.Err(); err != nil {
		return nil, err
	}

	// Recent entries
	recent, err := db.getQueryLogEntries(limit, actor)
	if err != nil {
		return nil, err
	}
	summary.RecentEntries = recent

	return summary, nil
}

// QueryLogPruneCount returns how many entries would be pruned if the table
// exceeds the configured query log row cap. Used by diagnostics/CLI.
func (db *DB) QueryLogPruneCount() (int, error) {
	var count int
	if err := db.conn.QueryRow("SELECT COUNT(*) FROM query_log").Scan(&count); err != nil {
		return 0, err
	}
	limit := db.effectiveQueryLogMaxEntries()
	if count <= limit {
		return 0, nil
	}
	return count - limit, nil
}
