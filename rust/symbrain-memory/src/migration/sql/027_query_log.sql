-- 027_query_log.sql
-- Bounded live query log for MCP tool usage tracking.

CREATE TABLE IF NOT EXISTS query_log (
    id          TEXT PRIMARY KEY,
    tool        TEXT NOT NULL,       -- which MCP tool was called
    query_text  TEXT,                -- the query/content passed to the tool
    params      TEXT,                -- optional truncated JSON of tool params
    duration_ms INTEGER NOT NULL DEFAULT 0,  -- execution time in milliseconds
    created_at  DATETIME NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_query_log_tool ON query_log(tool);
CREATE INDEX IF NOT EXISTS idx_query_log_created_at ON query_log(created_at);
