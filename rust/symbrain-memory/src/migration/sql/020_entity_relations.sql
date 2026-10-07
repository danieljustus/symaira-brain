CREATE TABLE IF NOT EXISTS entity_relations (
    from_entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    to_entity_id   TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    relation_type  TEXT NOT NULL,
    created_by     TEXT NOT NULL DEFAULT '',
    created_at     DATETIME NOT NULL,
    PRIMARY KEY (from_entity_id, to_entity_id, relation_type)
);

CREATE INDEX IF NOT EXISTS idx_entity_relations_to ON entity_relations(to_entity_id);
