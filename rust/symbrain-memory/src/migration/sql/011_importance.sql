-- 011_importance.sql
-- Adds an importance score to memories for composite retrieval ranking.

ALTER TABLE memories ADD COLUMN importance REAL NOT NULL DEFAULT 0.5;

CREATE INDEX idx_memories_importance ON memories(importance);
