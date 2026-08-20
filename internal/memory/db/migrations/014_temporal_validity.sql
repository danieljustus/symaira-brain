-- 014_temporal_validity.sql
-- Adds bi-temporal validity tracking to memories.

ALTER TABLE memories ADD COLUMN valid_from DATETIME;
ALTER TABLE memories ADD COLUMN valid_to DATETIME;
ALTER TABLE memories ADD COLUMN superseded_by TEXT;

CREATE INDEX idx_memories_valid_from ON memories(valid_from);
CREATE INDEX idx_memories_valid_to ON memories(valid_to);
CREATE INDEX idx_memories_superseded_by ON memories(superseded_by);
