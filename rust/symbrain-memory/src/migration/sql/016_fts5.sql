CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
    id UNINDEXED,
    content,
    scope,
    content=memories,
    content_rowid=rowid
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
