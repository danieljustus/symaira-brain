-- Memory governance (#485 staging, #486 semantic kind, #491 aging).
-- Existing rows stay live (review_status 'approved'), unclassified (kind ''),
-- and undecayed (decay_factor 1.0); no guesses are made about legacy data.
ALTER TABLE memories ADD COLUMN review_status TEXT NOT NULL DEFAULT 'approved';
ALTER TABLE memories ADD COLUMN kind TEXT NOT NULL DEFAULT '';
ALTER TABLE memories ADD COLUMN decay_factor REAL NOT NULL DEFAULT 1.0;
ALTER TABLE memories ADD COLUMN retired_at DATETIME;

CREATE INDEX IF NOT EXISTS idx_memories_review_status ON memories(review_status);
CREATE INDEX IF NOT EXISTS idx_memories_kind ON memories(kind);
