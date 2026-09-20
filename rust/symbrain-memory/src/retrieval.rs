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
//! One honest limit: the recency term is computed against the current clock, so
//! a result scored for an *almost new* memory can differ in its last `float32`
//! digit between two processes started milliseconds apart. The differential
//! fixture uses a memory dated months ago, where the term is six orders of
//! magnitude below one `float32` step.

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

/// One LSH-bucket candidate, before hydration.
struct Candidate {
    id: String,
    embedding_dim: i64,
}

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
    let buckets = buckets_for(query_vector)?;
    let candidates = candidates(conn, &buckets, query_source, scope)?;
    let candidates: Vec<Candidate> = candidates
        .into_iter()
        .filter(|candidate| Some(candidate.embedding_dim) == i64::try_from(query_vector.len()).ok())
        .collect();
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let ids: Vec<String> = candidates
        .into_iter()
        .map(|candidate| candidate.id)
        .collect();
    let mut hits = hydrate(conn, &ids, query_vector)?;
    score_and_rank(&mut hits, query_vector);
    hits.truncate(limit);
    track_access(conn, &hits)?;
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
             datetime('now')) AND review_status = 'approved' AND retired_at IS NULL \
             ORDER BY created_at DESC";
        let projection = "id, embedding_binary, embedding_dim";
        let (sql, mut arguments) = if scope.is_empty() {
            (
                format!("SELECT {projection} FROM memories WHERE {filter}{placeholders}{tail}"),
                Vec::new(),
            )
        } else {
            (
                format!(
                    "SELECT {projection} FROM memories WHERE scope = ? AND {filter}{placeholders}{tail}"
                ),
                vec![scope.to_owned()],
            )
        };
        arguments.push(query_source.to_owned());
        arguments.push(String::new());
        arguments.extend(chunk.iter().map(i64::to_string));

        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(arguments.iter()), |row| {
            Ok(Candidate {
                id: row.get(0)?,
                embedding_dim: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
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
fn cosine_similarity(left: &[f32], right: &[f32]) -> f32 {
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
