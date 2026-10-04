CREATE TABLE IF NOT EXISTS sync_oplog (
    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
    op TEXT NOT NULL CHECK (op IN ('upsert', 'delete')),
    memory_id TEXT NOT NULL,
    ts DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_sync_oplog_ts ON sync_oplog(ts);
CREATE INDEX IF NOT EXISTS idx_sync_oplog_memory ON sync_oplog(memory_id, event_id);

CREATE TRIGGER IF NOT EXISTS trg_memories_oplog_insert
AFTER INSERT ON memories
BEGIN
    INSERT INTO sync_oplog (op, memory_id) VALUES ('upsert', NEW.id);
END;

CREATE TRIGGER IF NOT EXISTS trg_memories_oplog_update
AFTER UPDATE ON memories
BEGIN
    INSERT INTO sync_oplog (op, memory_id) VALUES ('upsert', NEW.id);
END;

CREATE TRIGGER IF NOT EXISTS trg_memories_oplog_delete
AFTER DELETE ON memories
BEGIN
    INSERT INTO sync_oplog (op, memory_id) VALUES ('delete', OLD.id);
END;

CREATE TABLE IF NOT EXISTS sync_relay (
    id TEXT PRIMARY KEY,
    updated_at DATETIME NOT NULL,
    blob BLOB NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sync_relay_updated ON sync_relay(updated_at);
