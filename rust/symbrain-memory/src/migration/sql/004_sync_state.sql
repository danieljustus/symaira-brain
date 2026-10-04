CREATE TABLE IF NOT EXISTS sync_state (
    remote TEXT PRIMARY KEY,
    last_sync DATETIME NOT NULL
);
