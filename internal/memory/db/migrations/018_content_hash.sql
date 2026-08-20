-- Content-hash dedup: replace O(n) LIKE metadata scan with O(1) indexed lookup.
ALTER TABLE memories ADD COLUMN content_hash TEXT DEFAULT '';
CREATE INDEX IF NOT EXISTS idx_memories_content_hash ON memories(content_hash);
