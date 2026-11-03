//! Vector retrieval pipeline for `memory search` under the shipped defaults.
//!
//! The shipped `SearchMemories` runs with `DefaultRankingWeights()`: candidate
//! selection expands the query into adjacent LSH buckets, rows are hydrated
//! and scored by cosine similarity plus recency and importance, and the
//! returned ids are recorded as accesses. Two defaults keep this slice small
//! and are relied on deliberately: the Hamming prefilter is opt-in
//! (`hybrid_search.prefilter_enabled`, default off) and the spreading bonus is
//! opt-in (`ranking.spreading_weight`, default 0). Any dynamic memory
//! configuration keeps the whole command on the shipped implementation, so
//! only the default path has to be reproduced here.
//!
//! One honest limit: recency uses a wall-clock read in each process, so a score
//! near an `f32` rounding midpoint can differ between sequential Go/Rust runs.
//! The differential search fixture dates memories to 2000, making recency
//! smaller than an `f64` ULP before the result is narrowed to `f32`.

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use crate::model::StoreError;
use crate::search_rows::{SEARCH_COLUMNS, SearchHit, SearchRow};

/// Candidate ceiling of the shipped bucket scan.
const MAX_CANDIDATES: usize = 2000;

/// Bucket batch size of the shipped scan.
const BATCH_SIZE: usize = 64;

/// Bucket distance the shipped scan expands to.
const NEIGHBOR_DISTANCE: u32 = 2;

/// Default ranking weights (`DefaultRankingWeights`).
const RELEVANCE_WEIGHT: f64 = 0.6;
const RECENCY_WEIGHT: f64 = 0.2;
const IMPORTANCE_WEIGHT: f64 = 0.2;
const RECENCY_HALF_LIFE: f64 = 30.0;

/// One LSH-bucket candidate, before hydration: identity, the filter fields
/// and the sign-bit vector the shipped scan reads.
struct Candidate {
    id: String,
    embedding_dim: i64,
    /// Sign-bit vector; `None` for a NULL column.
    binary: Option<Vec<u8>>,
    metadata: serde_json::Map<String, serde_json::Value>,
    created_at: DateTime<Utc>,
    superseded_by: String,
}

/// `db.TrustFilter`.
#[derive(Debug, Clone, Default)]
pub struct TrustFilter {
    /// Minimum `confidence` metadata (`low`, `medium`, `high`).
    pub min_confidence: String,
    /// Required `verification_status` metadata.
    pub verification_status: String,
    /// Drop memories with a `superseded_by` link.
    pub exclude_superseded: bool,
    /// Maximum age of a memory.
    pub max_age: Option<std::time::Duration>,
}

/// `db.PolicyFilter`.
#[derive(Debug, Clone, Default)]
pub struct PolicyFilter {
    /// Highest admitted `sensitivity`.
    pub max_sensitivity: String,
    /// Lowest admitted `sharing_level`.
    pub min_sharing_level: String,
    /// Client the memory must be readable by.
    pub client_id: String,
}

/// The filters of `SearchMemoriesFilteredWithTrust` that candidate selection
/// applies before hydration.
#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    /// Resolved entity id; only linked memories qualify.
    pub entity_id: String,
    /// Trust filter.
    pub trust: TrustFilter,
    /// Policy filter.
    pub policy: PolicyFilter,
}

fn meta<'a>(candidate: &'a Candidate, key: &str) -> &'a str {
    candidate
        .metadata
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
}

fn rank(order: &[&str], value: &str, unknown: usize) -> usize {
    order
        .iter()
        .position(|name| *name == value)
        .unwrap_or(unknown)
}

fn csv(text: &str) -> impl Iterator<Item = &str> {
    text.split(',').map(str::trim).filter(|part| !part.is_empty())
}

impl SearchFilters {
    fn admits(&self, candidate: &Candidate) -> bool {
        let trust = &self.trust;
        if trust.exclude_superseded && !candidate.superseded_by.is_empty() {
            return false;
        }
        if !trust.min_confidence.is_empty() {
            let confidence = meta(candidate, "confidence");
            let confidence = if confidence.is_empty() { "medium" } else { confidence };
            let order = ["low", "medium", "high"];
            if rank(&order, confidence, 1) < rank(&order, &trust.min_confidence, 1) {
                return false;
            }
        }
        if !trust.verification_status.is_empty() {
            let status = meta(candidate, "verification_status");
            let status = if status.is_empty() { "unverified" } else { status };
            if status != trust.verification_status {
                return false;
            }
        }
        if let Some(max_age) = trust.max_age
            && !max_age.is_zero()
            && Utc::now()
                .signed_duration_since(candidate.created_at)
                .to_std()
                .is_ok_and(|age| age > max_age)
        {
            return false;
        }
        let policy = &self.policy;
        if !policy.max_sensitivity.is_empty() {
            let sensitivity = meta(candidate, "sensitivity");
            let sensitivity = if sensitivity.is_empty() { "internal" } else { sensitivity };
            let order = ["public", "internal", "confidential", "secret"];
            if rank(&order, sensitivity, 1) > rank(&order, &policy.max_sensitivity, 1) {
                return false;
            }
        }
        if !policy.min_sharing_level.is_empty() {
            let sharing = meta(candidate, "sharing_level");
            let sharing = if sharing.is_empty() { "private" } else { sharing };
            let order = ["private", "team", "org", "public"];
            if rank(&order, sharing, 0) < rank(&order, &policy.min_sharing_level, 0) {
                return false;
            }
        }
        if !policy.client_id.is_empty() {
            let denied = meta(candidate, "denied_clients");
            if csv(denied).any(|client| client == policy.client_id) {
                return false;
            }
            let allowed = meta(candidate, "allowed_clients");
            if !allowed.is_empty() && !csv(allowed).any(|client| client == policy.client_id) {
                return false;
            }
        }
        true
    }
}

/// Retrieval statistics sink (`RetrievalStats.Record`).
pub(crate) type Observe<'a> = Option<&'a dyn Fn(usize, f64, std::time::Duration)>;

/// Runs the shipped retrieval pipeline for a prepared query vector.
///
/// `query_source` is the embedding space of `query_vector`; rows from another
/// space are never hydrated or scored.
pub(crate) fn search(
    conn: &Connection,
    query_vector: &[f32],
    query_source: &str,
    scope: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, StoreError> {
    search_prefiltered(conn, query_vector, query_source, scope, limit, false)
}

/// `search` with the `hybrid_search.prefilter_enabled` Hamming prefilter.
pub(crate) fn search_prefiltered(
    conn: &Connection,
    query_vector: &[f32],
    query_source: &str,
    scope: &str,
    limit: usize,
    prefilter: bool,
) -> Result<Vec<SearchHit>, StoreError> {
    search_with(
        conn,
        &Query {
            vector: query_vector,
            source: query_source,
            scope,
            limit,
            prefilter,
            filters: &SearchFilters::default(),
            observe: None,
        },
    )
}

/// The public inputs of one service search.
pub struct ServiceSearch<'a> {
    /// Query vector.
    pub vector: &'a [f32],
    /// Embedding space of the vector.
    pub source: &'a str,
    /// Scope filter (empty: all).
    pub scope: &'a str,
    /// Result limit.
    pub limit: usize,
    /// `hybrid_search.prefilter_enabled`.
    pub prefilter: bool,
    /// Entity, trust and policy filters.
    pub filters: &'a SearchFilters,
    /// Retrieval statistics sink.
    pub observe: Option<&'a dyn Fn(usize, f64, std::time::Duration)>,
}

/// The inputs of one retrieval.
pub(crate) struct Query<'a> {
    pub vector: &'a [f32],
    pub source: &'a str,
    pub scope: &'a str,
    pub limit: usize,
    pub prefilter: bool,
    pub filters: &'a SearchFilters,
    pub observe: Observe<'a>,
}

/// The shipped `prefilterCandidates`: with the prefilter on, keep the
/// `max(4*limit, 64)` candidates closest in Hamming distance (ties keep the
/// earlier candidate); a candidate without a binary vector disables it.
fn prefilter_ids(candidates: &[Candidate], query_vector: &[f32], limit: usize) -> Vec<String> {
    let all = || candidates.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
    if candidates.is_empty() {
        return all();
    }
    let keep = limit.saturating_mul(4).max(64);
    // At or below the cut-off the shipped selection returns every index in
    // candidate order, without ranking by distance.
    if candidates.len() <= keep {
        return all();
    }
    let mut binaries = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        match &candidate.binary {
            Some(bytes) => binaries.push(bytes.as_slice()),
            None => return all(),
        }
    }
    let query = crate::cli_write::sign_bits(query_vector);
    let distance = |other: &[u8]| -> usize {
        if other.len() != query.len() {
            return 768;
        }
        query
            .chunks_exact(8)
            .zip(other.chunks_exact(8))
            .map(|(left, right)| {
                left.iter()
                    .zip(right)
                    .map(|(a, b)| (a ^ b).count_ones() as usize)
                    .sum::<usize>()
            })
            .sum()
    };
    let mut order: Vec<(usize, usize)> = binaries
        .iter()
        .enumerate()
        .map(|(index, bytes)| (distance(bytes), index))
        .collect();
    // (distance, index) is a total order, so a stable selection of the
    // smallest `keep` pairs equals the shipped insertion selection.
    order.sort_unstable();
    order.truncate(keep);
    order
        .into_iter()
        .map(|(_, index)| candidates[index].id.clone())
        .collect()
}

pub(crate) fn search_with(conn: &Connection, query: &Query<'_>) -> Result<Vec<SearchHit>, StoreError> {
    let started = std::time::Instant::now();
    let record = |hits: &[SearchHit]| {
        if let Some(observe) = query.observe {
            observe(
                hits.len(),
                hits.first().map_or(0.0, |hit| f64::from(hit.score)),
                started.elapsed(),
            );
        }
    };
    let buckets = buckets_for(query.vector)?;
    let candidates = candidates(conn, &buckets, query.source, query.scope, &query.filters.entity_id)?;
    let candidates: Vec<Candidate> = candidates
        .into_iter()
        .filter(|candidate| Some(candidate.embedding_dim) == i64::try_from(query.vector.len()).ok())
        .filter(|candidate| query.filters.admits(candidate))
        .collect();
    if candidates.is_empty() {
        record(&[]);
        return Ok(Vec::new());
    }

    let ids: Vec<String> = if query.prefilter {
        prefilter_ids(&candidates, query.vector, query.limit)
    } else {
        candidates
            .into_iter()
            .map(|candidate| candidate.id)
            .collect()
    };
    let mut hits = hydrate(conn, &ids, query.vector)?;
    score_and_rank(&mut hits, query.vector);
    hits.truncate(query.limit);
    record(&hits);
    // Access tracking is best effort in the shipped search.
    let _ = track_access(conn, &hits);
    Ok(hits)
}

/// Expands the query vector into its own bucket and every bucket within
/// [`NEIGHBOR_DISTANCE`] flips, the order the shipped scan uses.
fn buckets_for(query_vector: &[f32]) -> Result<Vec<i64>, StoreError> {
    let hash = crate::lsh::compute_lsh(query_vector)
        .map_err(|error| StoreError::Invalid(format!("search vector: {error}")))?;
    Ok(crate::lsh::lsh_neighbors(hash, NEIGHBOR_DISTANCE))
}

/// Reads the bucket candidates in the shipped column order and cap.
fn candidates(
    conn: &Connection,
    buckets: &[i64],
    query_source: &str,
    scope: &str,
    entity_id: &str,
) -> Result<Vec<Candidate>, StoreError> {
    let mut candidates = Vec::new();
    let mut start = 0;
    while start < buckets.len() && candidates.len() < MAX_CANDIDATES {
        let end = (start + BATCH_SIZE).min(buckets.len());
        let chunk = &buckets[start..end];
        let placeholders = placeholders(chunk.len());
        let filter = "consolidation_status != 'archived' AND embedding_source = ? AND \
             embedding_quantization = ? AND embedding IS NOT NULL AND lsh_hash IN (";
        let tail = ") AND (tier != 'working' OR expires_at IS NULL OR expires_at > \
             datetime('now')) AND review_status = 'approved' AND retired_at IS NULL";
        let entity = if entity_id.is_empty() {
            ""
        } else {
            " AND id IN (SELECT memory_id FROM memory_entities WHERE entity_id = ?)"
        };
        let projection = "id, embedding_binary, embedding_dim, metadata, created_at, valid_from, \
             valid_to, superseded_by";
        let (sql, mut arguments) = if scope.is_empty() {
            (
                format!(
                    "SELECT {projection} FROM memories WHERE {filter}{placeholders}{tail}{entity} ORDER BY created_at DESC"
                ),
                Vec::new(),
            )
        } else {
            (
                format!(
                    "SELECT {projection} FROM memories WHERE scope = ? AND {filter}{placeholders}{tail}{entity} ORDER BY created_at DESC"
                ),
                vec![scope.to_owned()],
            )
        };
        arguments.push(query_source.to_owned());
        arguments.push(String::new());
        arguments.extend(chunk.iter().map(i64::to_string));
        if !entity_id.is_empty() {
            arguments.push(entity_id.to_owned());
        }

        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(arguments.iter()), |row| {
            use crate::goscan::{blob, decode_metadata, null_int, null_text, null_time, raw, text, time};
            let id = text(row, 0, "id")?;
            let binary = blob(row, 1, "embedding_binary")?;
            let dim = null_int(row, 2, "embedding_dim")?.unwrap_or(0);
            let metadata_bytes = raw(row, 3, "metadata")?;
            let created_at = time(row, 4, "created_at")?;
            null_time(row, 5, "valid_from")?;
            null_time(row, 6, "valid_to")?;
            let superseded_by = null_text(row, 7, "superseded_by")?;
            let (metadata, _) = decode_metadata(&metadata_bytes)?;
            Ok(Candidate {
                id,
                embedding_dim: dim,
                binary,
                metadata,
                created_at,
                superseded_by,
            })
        })?;
        for row in rows {
            candidates.push(row?);
            if candidates.len() >= MAX_CANDIDATES {
                break;
            }
        }
        start = end;
    }
    Ok(candidates)
}

/// Loads the full rows for the candidate ids, restoring the candidate order
/// (SQLite does not guarantee the order of an `IN` list) and dropping rows
/// whose embedding does not fit the query.
fn hydrate(
    conn: &Connection,
    ids: &[String],
    query_vector: &[f32],
) -> Result<Vec<SearchHit>, StoreError> {
    let mut hits = Vec::new();
    for chunk in ids.chunks(BATCH_SIZE) {
        let sql = format!(
            "SELECT {SEARCH_COLUMNS} FROM memories WHERE id IN ({})",
            placeholders(chunk.len())
        );
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
            SearchRow::from_row(row)
        })?;
        let mut hydrated: Vec<(String, SearchRow)> = Vec::new();
        for row in rows {
            let memory = row?;
            if memory.embedding.is_empty() || memory.embedding.len() != query_vector.len() {
                continue;
            }
            hydrated.push((memory.id.clone(), memory));
        }
        for id in chunk {
            if let Some(position) = hydrated.iter().position(|(candidate, _)| candidate == id) {
                let (_, memory) = hydrated.remove(position);
                hits.push(SearchHit { memory, score: 0.0 });
            }
        }
    }
    Ok(hits)
}

/// Computes the composite score of every hit and sorts them descending.
fn score_and_rank(hits: &mut [SearchHit], query_vector: &[f32]) {
    let now = Utc::now();
    for hit in hits.iter_mut() {
        let relevance = cosine_similarity(query_vector, &hit.memory.embedding);
        let mut decay = hit.memory.decay_factor;
        if decay <= 0.0 || decay > 1.0 {
            decay = 1.0;
        }
        #[allow(clippy::cast_possible_truncation)]
        {
            hit.score =
                (composite_score(relevance, hit.memory.created_at, hit.memory.importance, now)
                    * decay) as f32;
        }
    }
    hits.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// The shipped `CompositeScore` under the default weights: relevance, recency
/// and importance, divided by their weight sum.
fn composite_score(
    relevance: f32,
    memory_age: Option<DateTime<Utc>>,
    importance: f64,
    now: DateTime<Utc>,
) -> f64 {
    let age_days = memory_age.map_or(0.0, |created| {
        let elapsed = now.signed_duration_since(created);
        #[allow(clippy::cast_precision_loss)]
        let nanos = elapsed.num_nanoseconds().unwrap_or(0) as f64;
        nanos / 1e9 / 86_400.0
    });
    let recency = (-0.693 * age_days / RECENCY_HALF_LIFE).exp();
    let total = RELEVANCE_WEIGHT + RECENCY_WEIGHT + IMPORTANCE_WEIGHT;
    (RELEVANCE_WEIGHT * f64::from(relevance)
        + RECENCY_WEIGHT * recency
        + IMPORTANCE_WEIGHT * (importance / 10.0))
        / total
}

/// The shipped `CosineSimilarity`, accumulated in `float64` and narrowed once.
pub(crate) fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    let mut dot_product = 0.0_f64;
    let mut norm_left = 0.0_f64;
    let mut norm_right = 0.0_f64;
    for (a, b) in left.iter().zip(right.iter()) {
        dot_product += f64::from(*a * *b);
        norm_left += f64::from(*a * *a);
        norm_right += f64::from(*b * *b);
    }
    if norm_left == 0.0 || norm_right == 0.0 {
        return 0.0;
    }
    #[allow(clippy::cast_possible_truncation)]
    let similarity = (dot_product / (norm_left.sqrt() * norm_right.sqrt())) as f32;
    similarity
}

/// Records the returned ids as accesses, exactly like the shipped feedback
/// loop: the count increments and the previous access time shifts back.
fn track_access(conn: &Connection, hits: &[SearchHit]) -> Result<(), StoreError> {
    if hits.is_empty() {
        return Ok(());
    }
    let now = crate::gotime::format(Utc::now());
    let ids: Vec<&str> = hits.iter().map(|hit| hit.memory.id.as_str()).collect();
    let sql = format!(
        "UPDATE memories SET access_count = access_count + 1, prev_access = last_access, \
         last_access = ? WHERE id IN ({})",
        placeholders(ids.len())
    );
    let mut arguments: Vec<String> = vec![now];
    arguments.extend(ids.into_iter().map(str::to_owned));
    conn.execute(&sql, rusqlite::params_from_iter(arguments.iter()))?;
    Ok(())
}

/// Builds a comma-separated `?` list of the given length.
fn placeholders(count: usize) -> String {
    let mut list = String::with_capacity(count.saturating_mul(2).saturating_sub(1));
    for index in 0..count {
        if index > 0 {
            list.push_str(", ");
        }
        list.push('?');
    }
    list
}

#[cfg(test)]
mod tests {
    use super::{composite_score, cosine_similarity, placeholders};

    #[test]
    fn placeholders_match_the_shipped_clause() {
        assert_eq!(placeholders(0), "");
        assert_eq!(placeholders(1), "?");
        assert_eq!(placeholders(3), "?, ?, ?");
    }

    #[test]
    fn cosine_is_one_for_identical_directions() {
        let vector = vec![1.0_f32, 2.0, 3.0];
        assert!((cosine_similarity(&vector, &vector) - 1.0).abs() < 1e-6);
        assert!((cosine_similarity(&vector, &[]) - 0.0).abs() < f32::EPSILON);
        assert!((cosine_similarity(&[0.0; 3], &vector) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_score_weights_the_three_default_terms() {
        // A memory created "now" contributes full recency, and importance is
        // scaled to a tenth before weighting.
        let now = chrono::Utc::now();
        let score = composite_score(1.0, Some(now), 10.0, now);
        assert!((score - 1.0).abs() < 1e-6, "score {score}");
    }
}
