CREATE INDEX IF NOT EXISTS idx_memories_updated_at ON memories(updated_at);
CREATE INDEX IF NOT EXISTS idx_memories_lsh ON memories(lsh_hash);
