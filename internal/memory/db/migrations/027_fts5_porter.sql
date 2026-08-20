-- Enable SQLite FTS5 porter tokenizer for stem-normalized BM25 keyword search.
-- The porter tokenizer applies the Porter stemming algorithm (e.g., "running",
-- "runs", "ran" all stem to "run"), improving recall for keyword search.
-- We drop and recreate the existing FTS5 table because SQLite does not support
-- ALTER TABLE for virtual tables; the triggers and content=memories external
-- content setup ensure no data is lost.

DROP TRIGGER IF EXISTS memories_ai;
DROP TRIGGER IF EXISTS memories_ad;
DROP TRIGGER IF EXISTS memories_au;

DROP TABLE IF EXISTS memories_fts;

CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
    id UNINDEXED,
    content,
    scope,
    content=memories,
    content_rowid=rowid,
    tokenize='porter unicode61'
);

CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
    INSERT INTO memories_fts(rowid, id, content, scope) VALUES (new.rowid, new.id, new.content, new.scope);
END;

CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, id, content, scope) VALUES('delete', old.rowid, old.id, old.content, old.scope);
END;

CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, id, content, scope) VALUES('delete', old.rowid, old.id, old.content, old.scope);
    INSERT INTO memories_fts(rowid, id, content, scope) VALUES (new.rowid, new.id, new.content, new.scope);
END;

-- Rebuild the FTS index from the external content table so existing memories
-- are immediately searchable with the new porter-stemmed tokenizer.
INSERT INTO memories_fts(memories_fts) VALUES('rebuild');
