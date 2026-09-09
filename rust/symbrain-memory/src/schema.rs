//! SQLite schema and migration identifiers mirrored from Go memory.

pub(crate) const SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS schema_migrations (version TEXT PRIMARY KEY, applied_at DATETIME DEFAULT CURRENT_TIMESTAMP);
CREATE TABLE IF NOT EXISTS memories (
 id TEXT PRIMARY KEY, content TEXT NOT NULL, scope TEXT NOT NULL DEFAULT '', metadata TEXT NOT NULL DEFAULT '{}',
 embedding TEXT NOT NULL DEFAULT '', created_at DATETIME NOT NULL, updated_at DATETIME NOT NULL,
 created_by TEXT NOT NULL DEFAULT '', updated_by TEXT NOT NULL DEFAULT '', created_session TEXT NOT NULL DEFAULT '',
 updated_session TEXT NOT NULL DEFAULT '', consolidation_status TEXT NOT NULL DEFAULT 'raw', importance REAL NOT NULL DEFAULT 0.5,
 valid_from DATETIME, valid_to DATETIME, superseded_by TEXT, embedding_source TEXT NOT NULL DEFAULT '', embedding_model TEXT NOT NULL DEFAULT '',
 content_hash TEXT DEFAULT '', tier TEXT NOT NULL DEFAULT 'long_term', expires_at DATETIME, access_count INTEGER NOT NULL DEFAULT 1,
 last_access DATETIME, review_status TEXT NOT NULL DEFAULT 'approved', kind TEXT NOT NULL DEFAULT '', decay_factor REAL NOT NULL DEFAULT 1.0,
 retired_at DATETIME, prev_access DATETIME
);
CREATE INDEX IF NOT EXISTS idx_memories_scope ON memories(scope);
CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at DESC, id DESC);
CREATE TABLE IF NOT EXISTS entities (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE, type TEXT NOT NULL DEFAULT 'person', aliases TEXT NOT NULL DEFAULT '[]', description TEXT NOT NULL DEFAULT '', created_by TEXT NOT NULL DEFAULT '', created_at DATETIME NOT NULL, updated_at DATETIME NOT NULL);
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
CREATE INDEX IF NOT EXISTS idx_entity_relations_to ON entity_relations(to_entity_id);
CREATE INDEX IF NOT EXISTS idx_entity_relations_id ON entity_relations(id);
CREATE TABLE IF NOT EXISTS profiles (id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE, type TEXT NOT NULL DEFAULT 'agent', role TEXT NOT NULL DEFAULT 'readwrite', description TEXT NOT NULL DEFAULT '', metadata TEXT NOT NULL DEFAULT '{}', created_at DATETIME NOT NULL, updated_at DATETIME NOT NULL);
CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, summary TEXT NOT NULL, updated_at DATETIME NOT NULL);
CREATE TABLE IF NOT EXISTS rules (id TEXT PRIMARY KEY, content TEXT NOT NULL, scope TEXT NOT NULL, metadata TEXT NOT NULL, created_at DATETIME NOT NULL);
CREATE TABLE IF NOT EXISTS query_log (id TEXT PRIMARY KEY, tool TEXT NOT NULL, query_text TEXT, params TEXT, duration_ms INTEGER NOT NULL DEFAULT 0, created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, actor TEXT, scope TEXT, session TEXT);
CREATE TABLE IF NOT EXISTS activity_segments (id TEXT PRIMARY KEY, source TEXT NOT NULL, granularity TEXT NOT NULL CHECK(granularity IN ('10min','6h')), started_at DATETIME NOT NULL, ended_at DATETIME NOT NULL, applications TEXT NOT NULL DEFAULT '[]', redacted_summary TEXT NOT NULL, raw_ref TEXT NOT NULL DEFAULT '', prior_segment_ids TEXT NOT NULL DEFAULT '[]', superseded_by TEXT NOT NULL DEFAULT '', expires_at DATETIME NOT NULL);
CREATE TABLE IF NOT EXISTS activity_episodes (id TEXT PRIMARY KEY, title TEXT NOT NULL, scope TEXT NOT NULL DEFAULT '', started_at DATETIME NOT NULL, ended_at DATETIME NOT NULL, confidence REAL NOT NULL CHECK(confidence >= 0 AND confidence <= 1), sources TEXT NOT NULL DEFAULT '[]', citations TEXT NOT NULL DEFAULT '[]', expires_at DATETIME NOT NULL);
CREATE INDEX IF NOT EXISTS idx_activity_segments_window ON activity_segments(started_at, ended_at);
CREATE INDEX IF NOT EXISTS idx_activity_episodes_window ON activity_episodes(started_at, ended_at);
";

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
