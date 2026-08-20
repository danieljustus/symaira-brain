-- Binary embedding column for Hamming-distance prefilter (issue #360).
-- Stores a 96-byte sign-bit vector (768 bits) derived from the float32 embedding.
-- Existing rows have NULL embedding_binary; they are populated on next save/update.
-- Search gracefully skips NULL entries and falls back to full cosine scoring.

ALTER TABLE memories ADD COLUMN embedding_binary BLOB;
