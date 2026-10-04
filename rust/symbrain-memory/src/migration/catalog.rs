//! Ordered Rust-owned copies of the 37 frozen Go migration effects.

pub(super) const STEPS: &[(&str, &str)] = &[
    ("001_init", include_str!("sql/001_init.sql")),
    ("002_vector_index", include_str!("sql/002_vector_index.sql")),
    (
        "003_jwt_revocations",
        include_str!("sql/003_jwt_revocations.sql"),
    ),
    ("004_sync_state", include_str!("sql/004_sync_state.sql")),
    ("005_indexes", include_str!("sql/005_indexes.sql")),
    ("006_provenance", include_str!("sql/006_provenance.sql")),
    ("007_profiles", include_str!("sql/007_profiles.sql")),
    ("008_entities", include_str!("sql/008_entities.sql")),
    (
        "009_consolidation",
        include_str!("sql/009_consolidation.sql"),
    ),
    ("010_import_state", include_str!("sql/010_import_state.sql")),
    ("011_importance", include_str!("sql/011_importance.sql")),
    ("012_audit_log", include_str!("sql/012_audit_log.sql")),
    (
        "014_temporal_validity",
        include_str!("sql/014_temporal_validity.sql"),
    ),
    (
        "015_entities_aliases",
        include_str!("sql/015_entities_aliases.sql"),
    ),
    ("016_fts5", include_str!("sql/016_fts5.sql")),
    (
        "017_embedding_source",
        include_str!("sql/017_embedding_source.sql"),
    ),
    ("018_content_hash", include_str!("sql/018_content_hash.sql")),
    (
        "019_memory_evidence",
        include_str!("sql/019_memory_evidence.sql"),
    ),
    (
        "020_entity_relations",
        include_str!("sql/020_entity_relations.sql"),
    ),
    (
        "021_context_profiles",
        include_str!("sql/021_context_profiles.sql"),
    ),
    (
        "022_entity_relation_provenance",
        include_str!("sql/022_entity_relation_provenance.sql"),
    ),
    ("023_sync_oplog", include_str!("sql/023_sync_oplog.sql")),
    (
        "024_working_memory_tier",
        include_str!("sql/024_working_memory_tier.sql"),
    ),
    (
        "025_entity_relation_temporal",
        include_str!("sql/025_entity_relation_temporal.sql"),
    ),
    (
        "026_binary_embedding",
        include_str!("sql/026_binary_embedding.sql"),
    ),
    ("027_access_count", include_str!("sql/027_access_count.sql")),
    (
        "027_consolidation_runs",
        include_str!("sql/027_consolidation_runs.sql"),
    ),
    ("027_fts5_porter", include_str!("sql/027_fts5_porter.sql")),
    ("027_query_log", include_str!("sql/027_query_log.sql")),
    (
        "027_retrieval_feedback",
        include_str!("sql/027_retrieval_feedback.sql"),
    ),
    (
        "028_embedding_quantization",
        include_str!("sql/028_embedding_quantization.sql"),
    ),
    (
        "029_query_log_attribution",
        include_str!("sql/029_query_log_attribution.sql"),
    ),
    ("030_audit_target", include_str!("sql/030_audit_target.sql")),
    (
        "031_query_log_results",
        include_str!("sql/031_query_log_results.sql"),
    ),
    (
        "032_memory_governance",
        include_str!("sql/032_memory_governance.sql"),
    ),
    (
        "033_recall_quality",
        include_str!("sql/033_recall_quality.sql"),
    ),
    (
        "034_activity_store",
        include_str!("sql/034_activity_store.sql"),
    ),
];
