-- Additive provenance for entity relations: a stable relation ID, an
-- optional caller-supplied source/source_ref for idempotent creation by
-- external integrations, a verification status, and bounded evidence JSON.
-- The existing (from_entity_id, to_entity_id, relation_type) primary key is
-- untouched, so current reads and writes keep working unchanged. Existing
-- rows are backfilled with a generated ID and empty provenance.

ALTER TABLE entity_relations ADD COLUMN id TEXT NOT NULL DEFAULT '';
ALTER TABLE entity_relations ADD COLUMN source TEXT NOT NULL DEFAULT '';
ALTER TABLE entity_relations ADD COLUMN source_ref TEXT NOT NULL DEFAULT '';
ALTER TABLE entity_relations ADD COLUMN verification TEXT NOT NULL DEFAULT '';
ALTER TABLE entity_relations ADD COLUMN evidence TEXT NOT NULL DEFAULT '';
ALTER TABLE entity_relations ADD COLUMN updated_at DATETIME;

UPDATE entity_relations
SET id = lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' ||
         substr(lower(hex(randomblob(2))), 2) || '-' ||
         substr('89ab', 1 + (abs(random()) % 4), 1) || substr(lower(hex(randomblob(2))), 2) || '-' ||
         lower(hex(randomblob(6)))
WHERE id = '';

UPDATE entity_relations SET updated_at = created_at WHERE updated_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_entity_relations_relation_id ON entity_relations(id);
CREATE INDEX IF NOT EXISTS idx_entity_relations_source ON entity_relations(source, source_ref);
