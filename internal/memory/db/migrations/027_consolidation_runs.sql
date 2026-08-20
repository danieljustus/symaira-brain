CREATE TABLE IF NOT EXISTS consolidation_runs (
    id TEXT PRIMARY KEY,
    run_at TEXT NOT NULL DEFAULT (datetime('now')),
    status TEXT NOT NULL DEFAULT 'completed' CHECK (status IN ('completed', 'undone')),
    summary_json TEXT NOT NULL,
    total_archived INTEGER NOT NULL DEFAULT 0,
    total_consolidated INTEGER NOT NULL DEFAULT 0
);
