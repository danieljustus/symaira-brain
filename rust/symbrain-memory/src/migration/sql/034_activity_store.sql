-- Bounded, provider-neutral activity history (#385).
-- Activity is deliberately separate from durable memories and has no sync
-- oplog triggers. Raw observations stay with their owning source; raw_ref is
-- only an opaque path or identifier.
CREATE TABLE IF NOT EXISTS activity_segments (
    id                 TEXT PRIMARY KEY,
    source             TEXT NOT NULL,
    granularity        TEXT NOT NULL CHECK (granularity IN ('10min', '6h')),
    started_at         DATETIME NOT NULL,
    ended_at           DATETIME NOT NULL,
    applications       TEXT NOT NULL DEFAULT '[]',
    redacted_summary   TEXT NOT NULL,
    raw_ref            TEXT NOT NULL DEFAULT '',
    prior_segment_ids  TEXT NOT NULL DEFAULT '[]',
    superseded_by     TEXT NOT NULL DEFAULT '',
    expires_at         DATETIME NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_activity_segments_window
    ON activity_segments (started_at, ended_at);
CREATE INDEX IF NOT EXISTS idx_activity_segments_expiry
    ON activity_segments (expires_at);
CREATE INDEX IF NOT EXISTS idx_activity_segments_source_granularity
    ON activity_segments (source, granularity, started_at);

CREATE TABLE IF NOT EXISTS activity_episodes (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    scope       TEXT NOT NULL DEFAULT '',
    started_at  DATETIME NOT NULL,
    ended_at    DATETIME NOT NULL,
    confidence  REAL NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
    sources     TEXT NOT NULL DEFAULT '[]',
    citations   TEXT NOT NULL DEFAULT '[]',
    expires_at  DATETIME NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_activity_episodes_window
    ON activity_episodes (started_at, ended_at);
CREATE INDEX IF NOT EXISTS idx_activity_episodes_expiry
    ON activity_episodes (expires_at);

-- #394's sync_exclude marker is the shared boundary used by activity-derived
-- staged memories. Keep it out of the oplog itself, including tombstones.
DROP TRIGGER IF EXISTS trg_memories_oplog_insert;
DROP TRIGGER IF EXISTS trg_memories_oplog_update;
DROP TRIGGER IF EXISTS trg_memories_oplog_delete;

CREATE TRIGGER trg_memories_oplog_insert
AFTER INSERT ON memories
WHEN COALESCE(json_extract(NEW.metadata, '$.sync_exclude'), '') != 'true'
BEGIN
    INSERT INTO sync_oplog (op, memory_id) VALUES ('upsert', NEW.id);
END;

CREATE TRIGGER trg_memories_oplog_update
AFTER UPDATE ON memories
WHEN COALESCE(json_extract(NEW.metadata, '$.sync_exclude'), '') != 'true'
BEGIN
    INSERT INTO sync_oplog (op, memory_id) VALUES ('upsert', NEW.id);
END;

CREATE TRIGGER trg_memories_oplog_delete
AFTER DELETE ON memories
WHEN COALESCE(json_extract(OLD.metadata, '$.sync_exclude'), '') != 'true'
BEGIN
    INSERT INTO sync_oplog (op, memory_id) VALUES ('delete', OLD.id);
END;
