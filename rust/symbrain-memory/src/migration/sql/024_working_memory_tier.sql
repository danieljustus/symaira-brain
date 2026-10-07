-- Working memory tier with TTL-based eviction.
-- tier: 'long_term' (default) or 'working' — working memories expire after expires_at.
-- expires_at: nullable DATETIME; when set and tier='working', the row is evictable after this time.
ALTER TABLE memories ADD COLUMN tier TEXT NOT NULL DEFAULT 'long_term';
ALTER TABLE memories ADD COLUMN expires_at DATETIME;

CREATE INDEX idx_memories_tier ON memories(tier);
CREATE INDEX idx_memories_expires_at ON memories(expires_at);
