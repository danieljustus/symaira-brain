ALTER TABLE memories ADD COLUMN embedding_dim INTEGER NOT NULL DEFAULT 0;
ALTER TABLE memories ADD COLUMN lsh_hash INTEGER NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_memories_scope_lsh ON memories(scope, lsh_hash);
