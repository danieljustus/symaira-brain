ALTER TABLE memories ADD COLUMN consolidation_status TEXT NOT NULL DEFAULT 'raw';
ALTER TABLE memories ADD COLUMN consolidated_into_id TEXT REFERENCES memories(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_memories_consolidation ON memories(consolidation_status);
CREATE INDEX IF NOT EXISTS idx_memories_consolidated_into ON memories(consolidated_into_id);
