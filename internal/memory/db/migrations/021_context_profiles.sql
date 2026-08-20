-- Context profiles: named, ordered scope-inheritance chains for memory retrieval.
-- Each profile has a base scope (the fallback scope when no links exist) and
-- zero or more links that define which scopes to search and in which order.
-- Links can optionally reference a parent context profile for nested inheritance.

CREATE TABLE IF NOT EXISTS context_profiles (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE COLLATE NOCASE,
    description TEXT NOT NULL DEFAULT '',
    base_scope  TEXT NOT NULL DEFAULT '',
    created_at  DATETIME NOT NULL,
    updated_at  DATETIME NOT NULL
);

CREATE TABLE IF NOT EXISTS context_profile_links (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id          TEXT NOT NULL REFERENCES context_profiles(id) ON DELETE CASCADE,
    parent_profile_id   TEXT REFERENCES context_profiles(id) ON DELETE SET NULL,
    scope               TEXT NOT NULL DEFAULT '',
    filter_key          TEXT NOT NULL DEFAULT '',
    filter_value        TEXT NOT NULL DEFAULT '',
    precedence_order    INTEGER NOT NULL DEFAULT 0,
    created_at          DATETIME NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_cpl_profile_id ON context_profile_links(profile_id);
CREATE INDEX IF NOT EXISTS idx_cpl_precedence ON context_profile_links(profile_id, precedence_order, id);
