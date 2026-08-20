-- 031_query_log_results.sql
-- Records which memories each retrieval returned (issue #460), so reads can
-- be traced back to the facts an agent relied on. One row per returned
-- memory, carrying its rank and score, linked to the query-log row. The
-- table stores references (memory ids), never a second copy of content.
-- Cascades keep the table free of dangling rows: pruning a query-log entry
-- removes its result rows, and deleting a memory removes its result rows.

CREATE TABLE IF NOT EXISTS query_log_results (
    query_id  TEXT NOT NULL,
    memory_id TEXT NOT NULL,
    rank      INTEGER NOT NULL,
    score     REAL NOT NULL,
    PRIMARY KEY (query_id, memory_id),
    FOREIGN KEY (query_id) REFERENCES query_log(id) ON DELETE CASCADE,
    FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_query_log_results_memory ON query_log_results(memory_id);
