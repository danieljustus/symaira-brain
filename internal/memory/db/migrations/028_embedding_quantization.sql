-- Embedding space identity (issue #439).
-- The identity of an embedding space comprises model, quantization level and
-- dimension. embedding_model and embedding_dim already exist; this migration
-- adds the quantization level. Empty string means the legacy unquantized
-- space, so existing rows stay in the same space they were written in.

ALTER TABLE memories ADD COLUMN embedding_quantization TEXT NOT NULL DEFAULT '';
