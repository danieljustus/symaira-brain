package db

import (
	"database/sql"
	"fmt"
	"strings"
	"sync/atomic"
	"time"

	"github.com/google/uuid"
)

// Audit event action type constants.
const (
	EventRedaction = "redaction"
)

// auditLogEnabled gates all audit writes. It defaults to enabled and is
// mirrored from the audit_log_enabled config value by the CLI and MCP entry
// points. When disabled, audit writes are skipped without error so that no
// operation ever fails because of audit logging.
var auditLogEnabled atomic.Bool

func init() {
	auditLogEnabled.Store(true)
}

// SetAuditLogEnabled toggles whether audit events are persisted. A disabled
// audit log never fails an operation: LogAudit and its callers return nil.
func (db *DB) SetAuditLogEnabled(enabled bool) {
	auditLogEnabled.Store(enabled)
}

type AuditEvent struct {
	ID         string    `json:"id"`
	Action     string    `json:"action"`
	MemoryID   string    `json:"memory_id,omitempty"`
	TargetType string    `json:"target_type,omitempty"`
	TargetID   string    `json:"target_id,omitempty"`
	Scope      string    `json:"scope,omitempty"`
	Session    string    `json:"session,omitempty"`
	Actor      string    `json:"actor,omitempty"`
	Detail     string    `json:"detail,omitempty"`
	CreatedAt  time.Time `json:"created_at"`
}

// Target types used with LogTargetAudit.
const (
	TargetEntity   = "entity"
	TargetRelation = "relation"
)

func (db *DB) LogAudit(action, memoryID, scope, session, actor, detail string) error {
	if !auditLogEnabled.Load() {
		return nil
	}
	if _, err := db.conn.Exec(
		`INSERT INTO audit_log (id, action, memory_id, scope, session, actor, detail, created_at)
		 VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
		uuid.New().String(), action, nullStr(memoryID), nullStr(scope),
		nullStr(session), nullStr(actor), nullStr(detail), time.Now().UTC(),
	); err != nil {
		return err
	}
	return nil
}

// LogRedactionAudit writes an audit event for a PII redaction. The patterns
// parameter contains the redaction pattern labels (e.g. "email", "api_key")
// that matched — never the raw matched values.
func (db *DB) LogRedactionAudit(memoryID, scope, session, actor string, patterns []string) error {
	detail := strings.Join(patterns, ",")
	return db.LogAudit(EventRedaction, memoryID, scope, session, actor, detail)
}

// LogTargetAudit writes an audit event for a non-memory target (entity or
// relation). The event's target_type/target_id columns carry the target;
// memory_id stays NULL so entity events are never mistaken for memory
// events when reading the log. Honors the audit_log_enabled switch like
// LogAudit.
func (db *DB) LogTargetAudit(action, targetType, targetID, scope, session, actor, detail string) error {
	if !auditLogEnabled.Load() {
		return nil
	}
	if _, err := db.conn.Exec(
		`INSERT INTO audit_log (id, action, target_type, target_id, memory_id, scope, session, actor, detail, created_at)
		 VALUES (?, ?, ?, ?, NULL, ?, ?, ?, ?, ?)`,
		uuid.New().String(), action, targetType, targetID,
		nullStr(scope), nullStr(session), nullStr(actor), nullStr(detail), time.Now().UTC(),
	); err != nil {
		return err
	}
	return nil
}

func (db *DB) GetAuditLogs(action string, limit int) ([]*AuditEvent, error) {
	if limit <= 0 {
		limit = 100
	}
	var rows *sql.Rows
	var err error
	if action != "" {
		rows, err = db.conn.Query(
			"SELECT id, action, memory_id, target_type, target_id, scope, session, actor, detail, created_at FROM audit_log WHERE action = ? ORDER BY created_at DESC LIMIT ?",
			action, limit,
		)
	} else {
		rows, err = db.conn.Query(
			"SELECT id, action, memory_id, target_type, target_id, scope, session, actor, detail, created_at FROM audit_log ORDER BY created_at DESC LIMIT ?",
			limit,
		)
	}
	if err != nil {
		return nil, err
	}
	defer func() { _ = rows.Close() }()

	var events []*AuditEvent
	for rows.Next() {
		var e AuditEvent
		var memID, tgtType, tgtID, sc, sess, act, det sql.NullString
		if err := rows.Scan(&e.ID, &e.Action, &memID, &tgtType, &tgtID, &sc, &sess, &act, &det, &e.CreatedAt); err != nil {
			return nil, err
		}
		e.MemoryID = memID.String
		e.TargetType = tgtType.String
		e.TargetID = tgtID.String
		e.Scope = sc.String
		e.Session = sess.String
		e.Actor = act.String
		e.Detail = det.String
		events = append(events, &e)
	}
	return events, nil
}

// PurgeExpiredMemories removes session-scoped memories older than the TTL
// and records a "purge" audit event for the batch. actor identifies the
// operator that requested the purge.
func (db *DB) PurgeExpiredMemories(ttl time.Duration, actor string) (int64, error) {
	cutoff := time.Now().UTC().Add(-ttl)
	result, err := db.conn.Exec(
		"DELETE FROM memories WHERE scope = 'session' AND created_at < ? AND consolidation_status != 'archived'",
		cutoff,
	)
	if err != nil {
		return 0, err
	}
	n, err := result.RowsAffected()
	if err != nil {
		return n, err
	}
	if n > 0 {
		_ = db.LogAudit("purge", "", "session", "", actor, fmt.Sprintf("purged=%d", n))
	}
	return n, nil
}

func (db *DB) PurgeExpiredSessions(ttl time.Duration) (int64, error) {
	cutoff := time.Now().UTC().Add(-ttl)
	result, err := db.conn.Exec(
		"DELETE FROM sessions WHERE updated_at < ?",
		cutoff,
	)
	if err != nil {
		return 0, err
	}
	return result.RowsAffected()
}

// PurgeByScope removes all non-archived memories in a scope and records a
// "purge" audit event for the batch. actor identifies the operator that
// requested the purge.
func (db *DB) PurgeByScope(scope, actor string) (int64, error) {
	result, err := db.conn.Exec(
		"DELETE FROM memories WHERE scope = ? AND consolidation_status != 'archived'",
		scope,
	)
	if err != nil {
		return 0, err
	}
	n, _ := result.RowsAffected()
	if n > 0 {
		_ = db.LogAudit("purge", "", scope, "", actor, fmt.Sprintf("purged=%d", n))
	}
	return n, nil
}

// PurgeByID removes a single memory by ID and records a "purge" audit event
// carrying the memory's scope and session at the time of the purge. actor
// identifies the operator that requested the purge.
func (db *DB) PurgeByID(id, actor string) (bool, error) {
	var scope, createdSession sql.NullString
	err := db.conn.QueryRow("SELECT scope, created_session FROM memories WHERE id = ?", id).Scan(&scope, &createdSession)
	if err == sql.ErrNoRows {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	result, err := db.conn.Exec("DELETE FROM memories WHERE id = ?", id)
	if err != nil {
		return false, err
	}
	n, _ := result.RowsAffected()
	if n > 0 {
		_ = db.LogAudit("purge", id, scope.String, createdSession.String, actor, "")
	}
	return n > 0, nil
}

// PurgeExpiredAuditLogs deletes audit events older than the retention
// window. It is the audit-side counterpart of the memory purge operations
// and honors the audit_retention config value.
func (db *DB) PurgeExpiredAuditLogs(retention time.Duration) (int64, error) {
	cutoff := time.Now().UTC().Add(-retention)
	result, err := db.conn.Exec("DELETE FROM audit_log WHERE created_at < ?", cutoff)
	if err != nil {
		return 0, err
	}
	return result.RowsAffected()
}

func nullStr(s string) interface{} {
	if s == "" {
		return nil
	}
	return s
}
