-- 012_audit_log.sql
-- Append-only audit log for memory mutations (GDPR compliance).

CREATE TABLE IF NOT EXISTS audit_log (
    id         TEXT PRIMARY KEY,
    action     TEXT NOT NULL,       -- set, delete, purge, sync
    memory_id  TEXT,                -- affected memory (nullable for bulk ops)
    scope      TEXT,                -- memory scope at time of action
    session    TEXT,                -- session that performed the action
    actor      TEXT,                -- who performed the action
    detail     TEXT,                -- optional JSON detail
    created_at DATETIME NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_audit_log_action ON audit_log(action);
CREATE INDEX idx_audit_log_memory_id ON audit_log(memory_id);
CREATE INDEX idx_audit_log_created_at ON audit_log(created_at);
