-- 029_query_log_attribution.sql
-- Query log attribution (issue #457).
-- Records which client asked (actor), in which scope and in which session
-- each query ran, so query log reads are attributable. All new columns are
-- nullable: rows written before this migration have no identity and keep
-- their existing data.

ALTER TABLE query_log ADD COLUMN actor TEXT;
ALTER TABLE query_log ADD COLUMN scope TEXT;
ALTER TABLE query_log ADD COLUMN session TEXT;

CREATE INDEX IF NOT EXISTS idx_query_log_actor ON query_log(actor);
