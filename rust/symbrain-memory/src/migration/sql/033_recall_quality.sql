-- Recall quality (#488 memory-to-memory associations, #489 spacing-aware reinforcement).
-- prev_access: the last_access value before the most recent reinforcement,
-- so the access curve can measure the gap since the previous recall.
ALTER TABLE memories ADD COLUMN prev_access DATETIME;

-- Weighted memory-to-memory associations. Edges are seeded from
-- co-retrieval in the query log, shared entity links, and consolidation
-- siblings; users can author additional edges via the CLI.
CREATE TABLE IF NOT EXISTS memory_associations (
    from_id    TEXT NOT NULL,
    to_id      TEXT NOT NULL,
    weight     REAL NOT NULL DEFAULT 1.0,
    created_by TEXT NOT NULL DEFAULT '',
    created_at DATETIME NOT NULL,
    PRIMARY KEY (from_id, to_id)
) WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_memory_associations_to ON memory_associations(to_id);
