-- Retrieval feedback loop (issue #409).
-- Track how often and when each memory is accessed for reinforcement ranking
-- and decay-aware consolidation.
-- access_count is already created by 027_access_count.sql (DEFAULT 1 for the
-- write-time deduplication counter); this migration only adds the timestamp.
-- Existing rows get NULL last_access.

ALTER TABLE memories ADD COLUMN last_access DATETIME;
