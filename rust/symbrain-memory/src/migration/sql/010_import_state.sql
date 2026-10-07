CREATE TABLE IF NOT EXISTS import_state (
    tool TEXT NOT NULL,
    session_id TEXT NOT NULL,
    imported_at DATETIME NOT NULL,
    memory_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (tool, session_id)
);

CREATE INDEX IF NOT EXISTS idx_import_state_tool ON import_state(tool);
CREATE INDEX IF NOT EXISTS idx_import_state_imported_at ON import_state(imported_at);
