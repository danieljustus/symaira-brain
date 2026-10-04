-- Embedding provenance: track which embedding source generated each vector
-- so that search and consolidation never cross-score incompatible spaces.
ALTER TABLE memories ADD COLUMN embedding_source TEXT NOT NULL DEFAULT '';
ALTER TABLE memories ADD COLUMN embedding_model TEXT NOT NULL DEFAULT '';
CREATE INDEX IF NOT EXISTS idx_memories_embedding_source ON memories(embedding_source);
