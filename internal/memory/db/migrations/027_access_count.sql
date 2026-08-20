-- Access-count column for write-time exact-content deduplication.
-- When a memory with the same content_hash already exists, the access_count
-- is incremented instead of inserting a new duplicate row.
-- The count starts at 1 for the initial write.
ALTER TABLE memories ADD COLUMN access_count INTEGER NOT NULL DEFAULT 1;
