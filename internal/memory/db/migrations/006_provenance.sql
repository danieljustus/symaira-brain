ALTER TABLE memories ADD COLUMN created_by TEXT NOT NULL DEFAULT '';
ALTER TABLE memories ADD COLUMN updated_by TEXT NOT NULL DEFAULT '';
ALTER TABLE memories ADD COLUMN created_session TEXT NOT NULL DEFAULT '';
ALTER TABLE memories ADD COLUMN updated_session TEXT NOT NULL DEFAULT '';

ALTER TABLE rules ADD COLUMN updated_at DATETIME;
ALTER TABLE rules ADD COLUMN created_by TEXT NOT NULL DEFAULT '';
ALTER TABLE rules ADD COLUMN updated_by TEXT NOT NULL DEFAULT '';

UPDATE rules SET updated_at = created_at WHERE updated_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_memories_created_by ON memories(created_by);
