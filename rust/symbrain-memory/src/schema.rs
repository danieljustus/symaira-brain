//! SQLite schema and migration identifiers mirrored from Go memory.

pub(crate) const SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS schema_migrations (version TEXT PRIMARY KEY, applied_at DATETIME DEFAULT CURRENT_TIMESTAMP);
CREATE TABLE IF NOT EXISTS memories (
 id TEXT PRIMARY KEY,
 content TEXT NOT NULL,
 scope TEXT NOT NULL,
 metadata TEXT NOT NULL,
 embedding TEXT NOT NULL,
 created_at DATETIME NOT NULL,
 updated_at DATETIME NOT NULL,
 embedding_dim INTEGER NOT NULL DEFAULT 0,
 lsh_hash INTEGER NOT NULL DEFAULT 0,
 created_by TEXT NOT NULL DEFAULT '',
 updated_by TEXT NOT NULL DEFAULT '',
 created_session TEXT NOT NULL DEFAULT '',
 updated_session TEXT NOT NULL DEFAULT '',
 consolidation_status TEXT NOT NULL DEFAULT 'raw',
 consolidated_into_id TEXT REFERENCES memories(id) ON DELETE SET NULL,
 importance REAL NOT NULL DEFAULT 0.5,
 valid_from DATETIME,
 valid_to DATETIME,
 superseded_by TEXT,
 embedding_source TEXT NOT NULL DEFAULT '',
 embedding_model TEXT NOT NULL DEFAULT '',
 content_hash TEXT DEFAULT '',
 tier TEXT NOT NULL DEFAULT 'long_term',
 expires_at DATETIME,
 embedding_binary BLOB,
 access_count INTEGER NOT NULL DEFAULT 1,
 last_access DATETIME,
 embedding_quantization TEXT NOT NULL DEFAULT '',
 review_status TEXT NOT NULL DEFAULT 'approved',
 kind TEXT NOT NULL DEFAULT '',
 decay_factor REAL NOT NULL DEFAULT 1.0,
 retired_at DATETIME,
 prev_access DATETIME
);
CREATE TABLE IF NOT EXISTS sync_oplog (
 event_id INTEGER PRIMARY KEY AUTOINCREMENT,
 op TEXT NOT NULL CHECK (op IN ('upsert', 'delete')),
 memory_id TEXT NOT NULL,
 ts DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE TABLE IF NOT EXISTS sync_relay (id TEXT PRIMARY KEY, updated_at DATETIME NOT NULL, blob BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS entities (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE, type TEXT NOT NULL DEFAULT 'person', aliases TEXT NOT NULL DEFAULT '[]', description TEXT NOT NULL DEFAULT '', created_by TEXT NOT NULL DEFAULT '', created_at DATETIME NOT NULL, updated_at DATETIME NOT NULL);
CREATE TABLE IF NOT EXISTS entities_aliases (
 entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
 alias TEXT NOT NULL,
 PRIMARY KEY (entity_id, alias)
);
CREATE TABLE IF NOT EXISTS memory_entities (memory_id TEXT NOT NULL, entity_id TEXT NOT NULL, PRIMARY KEY(memory_id, entity_id));
CREATE TABLE IF NOT EXISTS entity_relations (
 from_entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
 to_entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
 relation_type TEXT NOT NULL, created_by TEXT NOT NULL DEFAULT '', created_at DATETIME NOT NULL,
 id TEXT NOT NULL DEFAULT '', source TEXT NOT NULL DEFAULT '', source_ref TEXT NOT NULL DEFAULT '',
 verification TEXT NOT NULL DEFAULT '', evidence TEXT NOT NULL DEFAULT '', updated_at DATETIME,
 valid_from DATETIME, valid_until DATETIME,
 PRIMARY KEY(from_entity_id, to_entity_id, relation_type)
);
CREATE TABLE IF NOT EXISTS profiles (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE, type TEXT NOT NULL DEFAULT 'agent', role TEXT NOT NULL DEFAULT 'readwrite', description TEXT NOT NULL DEFAULT '', metadata TEXT NOT NULL DEFAULT '{}', created_at DATETIME NOT NULL, updated_at DATETIME NOT NULL);
CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, summary TEXT NOT NULL, updated_at DATETIME NOT NULL);
CREATE TABLE IF NOT EXISTS rules (id TEXT PRIMARY KEY, content TEXT NOT NULL, scope TEXT NOT NULL, metadata TEXT NOT NULL, created_at DATETIME NOT NULL, updated_at DATETIME, created_by TEXT NOT NULL DEFAULT '', updated_by TEXT NOT NULL DEFAULT '');
CREATE TABLE IF NOT EXISTS query_log (id TEXT PRIMARY KEY, tool TEXT NOT NULL, query_text TEXT, params TEXT, duration_ms INTEGER NOT NULL DEFAULT 0, created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, actor TEXT, scope TEXT, session TEXT);
CREATE TABLE IF NOT EXISTS query_log_results (
 query_id TEXT NOT NULL,
 memory_id TEXT NOT NULL,
 rank INTEGER NOT NULL,
 score REAL NOT NULL,
 PRIMARY KEY (query_id, memory_id),
 FOREIGN KEY (query_id) REFERENCES query_log(id) ON DELETE CASCADE,
 FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS memory_evidence (
 id TEXT PRIMARY KEY,
 memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
 source_id TEXT NOT NULL DEFAULT '',
 source_kind TEXT NOT NULL DEFAULT '',
 text TEXT NOT NULL DEFAULT '',
 evidence_text TEXT NOT NULL,
 char_start INTEGER NOT NULL,
 char_end INTEGER NOT NULL,
 alignment_status TEXT NOT NULL,
 created_at DATETIME NOT NULL
);
CREATE TABLE IF NOT EXISTS audit_log (
 id TEXT PRIMARY KEY,
 action TEXT NOT NULL,
 memory_id TEXT,
 scope TEXT,
 session TEXT,
 actor TEXT,
 detail TEXT,
 created_at DATETIME NOT NULL DEFAULT (datetime('now')),
 target_type TEXT,
 target_id TEXT
);
CREATE TABLE IF NOT EXISTS consolidation_runs (
 id TEXT PRIMARY KEY,
 run_at TEXT NOT NULL DEFAULT (datetime('now')),
 status TEXT NOT NULL DEFAULT 'completed' CHECK (status IN ('completed', 'undone')),
 summary_json TEXT NOT NULL,
 total_archived INTEGER NOT NULL DEFAULT 0,
 total_consolidated INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS context_profiles (
 id TEXT PRIMARY KEY,
 name TEXT NOT NULL UNIQUE COLLATE NOCASE,
 description TEXT NOT NULL DEFAULT '',
 base_scope TEXT NOT NULL DEFAULT '',
 created_at DATETIME NOT NULL,
 updated_at DATETIME NOT NULL
);
CREATE TABLE IF NOT EXISTS context_profile_links (
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 profile_id TEXT NOT NULL REFERENCES context_profiles(id) ON DELETE CASCADE,
 parent_profile_id TEXT REFERENCES context_profiles(id) ON DELETE SET NULL,
 scope TEXT NOT NULL DEFAULT '',
 filter_key TEXT NOT NULL DEFAULT '',
 filter_value TEXT NOT NULL DEFAULT '',
 precedence_order INTEGER NOT NULL DEFAULT 0,
 created_at DATETIME NOT NULL
);
CREATE TABLE IF NOT EXISTS import_state (
 tool TEXT NOT NULL,
 session_id TEXT NOT NULL,
 imported_at DATETIME NOT NULL,
 memory_count INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY (tool, session_id)
);
CREATE TABLE IF NOT EXISTS jwt_revocations (
 jti TEXT PRIMARY KEY,
 revoked_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
 id UNINDEXED,
 content,
 scope,
 content=memories,
 content_rowid=rowid,
 tokenize='porter unicode61'
);
CREATE TABLE IF NOT EXISTS 'memories_fts_config'(k PRIMARY KEY, v) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS 'memories_fts_data'(id INTEGER PRIMARY KEY, block BLOB);
CREATE TABLE IF NOT EXISTS 'memories_fts_docsize'(id INTEGER PRIMARY KEY, sz BLOB);
CREATE TABLE IF NOT EXISTS 'memories_fts_idx'(segid, term, pgno, PRIMARY KEY(segid, term)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS memory_associations (
 from_id TEXT NOT NULL,
 to_id TEXT NOT NULL,
 weight REAL NOT NULL DEFAULT 1.0,
 created_by TEXT NOT NULL DEFAULT '',
 created_at DATETIME NOT NULL,
 PRIMARY KEY (from_id, to_id)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS sync_state (
 remote TEXT PRIMARY KEY,
 last_sync DATETIME NOT NULL
);
CREATE TRIGGER IF NOT EXISTS trg_memories_oplog_insert
AFTER INSERT ON memories
WHEN COALESCE(json_extract(NEW.metadata, '$.sync_exclude'), '') != 'true'
BEGIN
 INSERT INTO sync_oplog (op, memory_id) VALUES ('upsert', NEW.id);
END;
CREATE TRIGGER IF NOT EXISTS trg_memories_oplog_update
AFTER UPDATE ON memories
WHEN COALESCE(json_extract(NEW.metadata, '$.sync_exclude'), '') != 'true'
BEGIN
 INSERT INTO sync_oplog (op, memory_id) VALUES ('upsert', NEW.id);
END;
CREATE TRIGGER IF NOT EXISTS trg_memories_oplog_delete
AFTER DELETE ON memories
WHEN COALESCE(json_extract(OLD.metadata, '$.sync_exclude'), '') != 'true'
BEGIN
 INSERT INTO sync_oplog (op, memory_id) VALUES ('delete', OLD.id);
END;
CREATE TABLE IF NOT EXISTS activity_segments (id TEXT PRIMARY KEY, source TEXT NOT NULL, granularity TEXT NOT NULL CHECK (granularity IN ('10min', '6h')), started_at DATETIME NOT NULL, ended_at DATETIME NOT NULL, applications TEXT NOT NULL DEFAULT '[]', redacted_summary TEXT NOT NULL, raw_ref TEXT NOT NULL DEFAULT '', prior_segment_ids TEXT NOT NULL DEFAULT '[]', superseded_by TEXT NOT NULL DEFAULT '', expires_at DATETIME NOT NULL);
CREATE TABLE IF NOT EXISTS activity_episodes (id TEXT PRIMARY KEY, title TEXT NOT NULL, scope TEXT NOT NULL DEFAULT '', started_at DATETIME NOT NULL, ended_at DATETIME NOT NULL, confidence REAL NOT NULL CHECK (confidence >= 0 AND confidence <= 1), sources TEXT NOT NULL DEFAULT '[]', citations TEXT NOT NULL DEFAULT '[]', expires_at DATETIME NOT NULL);
CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
    INSERT INTO memories_fts(rowid, id, content, scope) VALUES (new.rowid, new.id, new.content, new.scope);
END;
CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, id, content, scope) VALUES('delete', old.rowid, old.id, old.content, old.scope);
END;
CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, id, content, scope) VALUES('delete', old.rowid, old.id, old.content, old.scope);
    INSERT INTO memories_fts(rowid, id, content, scope) VALUES (new.rowid, new.id, new.content, new.scope);
END;
";

/// Columns the shipped schema owns that an older native database may lack.
pub(crate) const COLUMN_PARITY: &[(&str, &str, &str)] = &[
    ("memories", "embedding_dim", "INTEGER NOT NULL DEFAULT 0"),
    ("memories", "lsh_hash", "INTEGER NOT NULL DEFAULT 0"),
    (
        "memories",
        "consolidated_into_id",
        "TEXT REFERENCES memories(id) ON DELETE SET NULL",
    ),
    ("memories", "embedding_binary", "BLOB"),
    (
        "memories",
        "embedding_quantization",
        "TEXT NOT NULL DEFAULT ''",
    ),
    ("memories", "access_count", "INTEGER NOT NULL DEFAULT 1"),
    (
        "memories",
        "consolidation_status",
        "TEXT NOT NULL DEFAULT 'raw'",
    ),
    ("memories", "content_hash", "TEXT DEFAULT ''"),
    ("memories", "created_by", "TEXT NOT NULL DEFAULT ''"),
    ("memories", "created_session", "TEXT NOT NULL DEFAULT ''"),
    ("memories", "decay_factor", "REAL NOT NULL DEFAULT 1.0"),
    ("memories", "embedding_model", "TEXT NOT NULL DEFAULT ''"),
    ("memories", "embedding_source", "TEXT NOT NULL DEFAULT ''"),
    ("memories", "expires_at", "DATETIME"),
    ("memories", "importance", "REAL NOT NULL DEFAULT 0.5"),
    ("memories", "kind", "TEXT NOT NULL DEFAULT ''"),
    ("memories", "last_access", "DATETIME"),
    ("memories", "prev_access", "DATETIME"),
    ("memories", "retired_at", "DATETIME"),
    (
        "memories",
        "review_status",
        "TEXT NOT NULL DEFAULT 'approved'",
    ),
    ("memories", "superseded_by", "TEXT"),
    ("memories", "tier", "TEXT NOT NULL DEFAULT 'long_term'"),
    ("memories", "updated_by", "TEXT NOT NULL DEFAULT ''"),
    ("memories", "updated_session", "TEXT NOT NULL DEFAULT ''"),
    ("memories", "valid_from", "DATETIME"),
    ("memories", "valid_to", "DATETIME"),
    ("rules", "updated_at", "DATETIME"),
    ("rules", "created_by", "TEXT NOT NULL DEFAULT ''"),
    ("rules", "updated_by", "TEXT NOT NULL DEFAULT ''"),
];

pub(crate) const MIGRATIONS: &[&str] = &[
    "001_init",
    "002_vector_index",
    "003_jwt_revocations",
    "004_sync_state",
    "005_indexes",
    "006_provenance",
    "007_profiles",
    "008_entities",
    "009_consolidation",
    "010_import_state",
    "011_importance",
    "012_audit_log",
    "014_temporal_validity",
    "015_entities_aliases",
    "016_fts5",
    "017_embedding_source",
    "018_content_hash",
    "019_memory_evidence",
    "020_entity_relations",
    "021_context_profiles",
    "022_entity_relation_provenance",
    "023_sync_oplog",
    "024_working_memory_tier",
    "025_entity_relation_temporal",
    "026_binary_embedding",
    "027_access_count",
    "027_consolidation_runs",
    "027_fts5_porter",
    "027_query_log",
    "027_retrieval_feedback",
    "028_embedding_quantization",
    "029_query_log_attribution",
    "030_audit_target",
    "031_query_log_results",
    "032_memory_governance",
    "033_recall_quality",
    "034_activity_store",
];

/// Indexes over `memories` columns that older databases only gain through
/// `COLUMN_PARITY`: they are applied after the repair so a legacy database
/// does not fail on a column it has not been given yet.
pub(crate) const INDEXES: &str = r"
CREATE INDEX IF NOT EXISTS idx_memories_scope ON memories(scope);
CREATE INDEX IF NOT EXISTS idx_memories_kind ON memories(kind);
CREATE INDEX IF NOT EXISTS idx_memories_tier ON memories(tier);
CREATE INDEX IF NOT EXISTS idx_memories_review_status ON memories(review_status);
CREATE INDEX IF NOT EXISTS idx_memories_expires_at ON memories(expires_at);
CREATE INDEX IF NOT EXISTS idx_memories_content_hash ON memories(content_hash);
CREATE INDEX IF NOT EXISTS idx_memories_embedding_source ON memories(embedding_source);
CREATE INDEX IF NOT EXISTS idx_memories_consolidation ON memories(consolidation_status);
CREATE INDEX IF NOT EXISTS idx_memories_consolidated_into ON memories(consolidated_into_id);
CREATE INDEX IF NOT EXISTS idx_memories_created_by ON memories(created_by);
CREATE INDEX IF NOT EXISTS idx_memories_importance ON memories(importance);
CREATE INDEX IF NOT EXISTS idx_memories_lsh ON memories(lsh_hash);
CREATE INDEX IF NOT EXISTS idx_memories_updated_at ON memories(updated_at);
CREATE INDEX IF NOT EXISTS idx_memories_scope_lsh ON memories(scope, lsh_hash);
CREATE INDEX IF NOT EXISTS idx_memories_valid_from ON memories(valid_from);
CREATE INDEX IF NOT EXISTS idx_memories_valid_to ON memories(valid_to);
CREATE INDEX IF NOT EXISTS idx_memories_superseded_by ON memories(superseded_by);
CREATE INDEX IF NOT EXISTS idx_sync_oplog_ts ON sync_oplog(ts);
CREATE INDEX IF NOT EXISTS idx_sync_oplog_memory ON sync_oplog(memory_id, event_id);
CREATE INDEX IF NOT EXISTS idx_sync_relay_updated ON sync_relay(updated_at);
CREATE INDEX IF NOT EXISTS idx_entity_relations_to ON entity_relations(to_entity_id);
CREATE INDEX IF NOT EXISTS idx_entity_relations_id ON entity_relations(id);
CREATE INDEX IF NOT EXISTS idx_activity_segments_window ON activity_segments(started_at, ended_at);
CREATE INDEX IF NOT EXISTS idx_activity_segments_expiry ON activity_segments(expires_at);
CREATE INDEX IF NOT EXISTS idx_activity_segments_source_granularity ON activity_segments(source, granularity, started_at);
CREATE INDEX IF NOT EXISTS idx_activity_episodes_window ON activity_episodes(started_at, ended_at);
CREATE INDEX IF NOT EXISTS idx_activity_episodes_expiry ON activity_episodes(expires_at);
";
