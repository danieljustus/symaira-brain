CREATE TABLE IF NOT EXISTS entities_aliases (
    entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    alias     TEXT NOT NULL,
    PRIMARY KEY (entity_id, alias)
);

CREATE INDEX IF NOT EXISTS idx_entities_aliases_lookup ON entities_aliases(alias COLLATE NOCASE);
