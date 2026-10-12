//! Opt-in CLI Hamming selection, after candidate scans and before hydration.

use super::{Candidate, SearchMode, search_inner};
use crate::search_rows::{SEARCH_COLUMNS, SearchHit, SearchRow};
use crate::{Store, StoreError};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

impl Store {
    /// Checks only the additional existing-store prefilter read boundary.
    /// The CLI must also retain its conservative JSON read admission.
    #[must_use]
    pub fn direct_prefilter_supported(path: &Path) -> bool {
        // The CLI flag contract is i64; only the supported 64-bit Go/Rust
        // targets have matching machine-int overflow. No new-store cutover.
        usize::BITS == 64 && path.is_file()
            && Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .is_ok_and(|conn| admission(&conn).is_ok())
    }

    /// Runs opt-in Hamming retrieval with the shipped CLI's default weights.
    /// Existing `search_ranked` and service/conflict callers are unchanged.
    ///
    /// # Errors
    /// Returns a `StoreError` for an unrepresentable Go int, invalid vector,
    /// or failed scan/hydration. Access feedback remains best-effort.
    pub fn search_ranked_prefiltered(
        &self,
        query_vector: &[f32],
        query_source: &str,
        scope: &str,
        limit: i64,
    ) -> Result<Vec<SearchHit>, StoreError> {
        let signed = isize::try_from(limit)
            .map_err(|_| StoreError::Invalid("search limit requires the Go pipeline".into()))?;
        let conn = self.lock()?;
        search_inner(
            &conn, query_vector, query_source, scope,
            usize::try_from(signed).unwrap_or(0), |_| true, SearchMode::Prefilter(signed),
        ).map_err(|error| match error {
            // The strict decoder already carries the original Go diagnostic.
            // Unwrap only this typed cause; legacy callers keep their errors.
            StoreError::Sql(rusqlite::Error::FromSqlConversionFailure(_, _, cause)) =>
                StoreError::Invalid(cause.to_string()),
            other => other,
        })
    }
}

fn admission(conn: &Connection) -> Result<(), StoreError> {
    // ponytail: retain whole-table admission, even for excluded rows, until
    // a separately reviewed malformed-read/domain cutover is authorized.
    let mut query = conn.prepare(&format!("SELECT {SEARCH_COLUMNS}, embedding_dim FROM memories"))?;
    for row in query.query_map([], |row| {
        let memory = SearchRow::from_conflict_row(row)?;
        let _: Option<i64> = row.get(30)?;
        Ok(memory)
    })? {
        let _ = row?;
    }
    Ok(())
}

pub(super) fn select(candidates: Vec<Candidate>, vector: &[f32], limit: isize) -> Vec<String> {
    // Go multiplies a signed machine int *before* applying either bound.
    let count = usize::try_from(limit.wrapping_mul(4).max(64))
        .unwrap_or(64).min(candidates.len());
    if count == candidates.len() || candidates.iter().any(|row| row.embedding_binary.is_none()) {
        return candidates.into_iter().map(|row| row.id).collect();
    }
    let query = sign_bits(vector);
    let mut ranked = candidates.into_iter().enumerate().map(|(index, row)| {
        let distance = hamming(&query, row.embedding_binary.as_deref().expect("checked non-NULL"));
        (distance, index, row.id)
    }).collect::<Vec<_>>();
    // Original index is the tie breaker, NOT the ID or final-score order.
    // At most 2000 candidates: a full sort is simpler than Go's insertion top-N.
    ranked.sort_unstable_by_key(|(distance, index, _)| (*distance, *index));
    ranked.into_iter().take(count).map(|(_, _, id)| id).collect()
}

fn sign_bits(vector: &[f32]) -> Vec<u8> {
    // ponytail: identical to the frozen private cli_write::sign_bits helper;
    // that module is read-only in this slice. Share it on an authorized delta.
    let mut bytes = vec![0_u8; 96];
    for (index, value) in vector.iter().take(768).enumerate() {
        if *value <= 0.0 {
            bytes[index / 8] |= 1 << (index % 8);
        }
    }
    bytes
}

fn hamming(left: &[u8], right: &[u8]) -> u32 {
    if left.len() != right.len() {
        return 768;
    }
    // Go ignores an incomplete final uint64, rather than counting every byte.
    left.chunks_exact(8).zip(right.chunks_exact(8)).map(|(a, b)| {
        let a = u64::from_le_bytes(a.try_into().expect("complete word"));
        let b = u64::from_le_bytes(b.try_into().expect("complete word"));
        (a ^ b).count_ones()
    }).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    #[test]
    fn selection_preserves_sign_words_nil_ties_and_signed_overflow() {
        let binary = sign_bits(&[1.0, -1.0, 0.0, -0.0, f32::NAN]);
        assert_eq!(binary[0], 0b0000_1110);
        assert_eq!(binary.len(), 96);
        assert!(binary[1..].iter().all(|byte| *byte == 0));
        assert_eq!(sign_bits(&[]), vec![0; 96]);
        assert_eq!(sign_bits(&[-1.0; 769]), vec![255; 96]);
        for (a, b, distance) in [
            (vec![], vec![], 0), (vec![], vec![0], 768),
            (vec![0; 7], vec![255; 7], 0),
            (vec![0; 9], vec![255; 9], 64),
            (vec![0; 96], vec![255; 96], 768),
        ] {
            assert_eq!(hamming(&a, &b), distance);
        }
        let rows = || (0..80).map(|index| Candidate {
            id: index.to_string(), embedding_dim: 768,
            embedding_binary: Some(vec![if index < 16 { 255 } else { 0 }; 96]),
        }).collect::<Vec<_>>();
        let original = (0..80).map(|i| i.to_string()).collect::<Vec<_>>();
        let selected = (16..80).map(|i| i.to_string()).collect::<Vec<_>>();
        for limit in [0, -1, 1, 16, isize::MIN, isize::MAX, isize::MAX / 4 + 1] {
            assert_eq!(select(rows(), &[1.0; 768], limit), selected);
        }
        assert_eq!(select(rows(), &[1.0; 768], 20), original);
        let mut nil = rows();
        nil[79].embedding_binary = None;
        assert_eq!(select(nil, &[1.0; 768], 1), original);
        for bytes in [vec![], vec![0; 7], vec![0; 95], vec![0; 97]] {
            let mut short = rows();
            short[16].embedding_binary = Some(bytes);
            let chosen = select(short, &[1.0; 768], 1);
            assert_eq!(&chosen[..63], &(17..80).map(|i| i.to_string()).collect::<Vec<_>>());
            assert_eq!(chosen[63], "0");
        }
        assert!(select(vec![], &[], 5).is_empty());
    }

    #[test]
    fn production_prefilter_selects_before_hydration_and_feedback() {
        let store = Store::open_in_memory().unwrap();
        let vector = vec![0.25; 768];
        let embedding = serde_json::to_string(&vector).unwrap();
        let lsh = crate::lsh::compute_lsh(&vector).unwrap();
        {
            let conn = store.lock().unwrap();
            for index in 0..80 {
                conn.execute(
                    "INSERT INTO memories(id,content,scope,metadata,embedding,embedding_binary,embedding_dim,embedding_source,embedding_model,embedding_quantization,lsh_hash,created_at,updated_at,created_by,updated_by,created_session,updated_session,importance,tier,review_status,kind,decay_factor,access_count) VALUES(?,?,'global','{}',?,?,768,'ollama','test','',?,'2000-01-01 00:00:00 +0000 UTC','2000-01-01 00:00:00 +0000 UTC','','','','',?,'long_term','approved','reference',1,0)",
                    params![format!("m-{index:03}"), format!("candidate {index}"), embedding,
                        vec![if index < 16 { 255_u8 } else { 0 }; 96], lsh, f64::from(80 - index)],
                ).unwrap();
            }
        }
        let ordinary = store.search_ranked(&vector, "ollama", "global", 5).unwrap();
        assert_eq!(ordinary[0].memory.id, "m-000");
        {
            let conn = store.lock().unwrap();
            conn.execute("UPDATE memories SET access_count=0,last_access=NULL,prev_access=NULL", []).unwrap();
            // HTTP's admission veto still precedes feedback on its old path.
            assert!(super::super::search_checked(&conn, &vector, "ollama", "global", 5, |_| false).is_err());
            assert_eq!(conn.query_row("SELECT sum(access_count) FROM memories", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
        }
        let hits = store.search_ranked_prefiltered(&vector, "ollama", "global", 5).unwrap();
        assert_eq!(hits.iter().map(|hit| hit.memory.id.as_str()).collect::<Vec<_>>(),
            ["m-016", "m-017", "m-018", "m-019", "m-020"]);
        {
            let conn = store.lock().unwrap();
            assert_eq!(conn.query_row("SELECT sum(access_count) FROM memories", [], |row| row.get::<_, i64>(0)).unwrap(), 5);
            assert_eq!(conn.query_row("SELECT sum(access_count) FROM memories WHERE id < 'm-016'", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
            // A bad full embedding in an excluded row is not hydrated; a bad
            // projected metadata/time field still fails before Hamming selection.
            conn.execute("UPDATE memories SET embedding='bad json' WHERE id='m-000'", []).unwrap();
        }
        assert!(store.search_ranked_prefiltered(&vector, "ollama", "global", 5).is_ok());
        assert!(store.search_ranked(&vector, "ollama", "global", 5).is_err());
        {
            let conn = store.lock().unwrap();
            conn.execute("UPDATE memories SET embedding='bad json' WHERE id='m-016'", []).unwrap();
        }
        let error = store.search_ranked_prefiltered(&vector, "ollama", "global", 5).unwrap_err();
        assert_eq!(error.to_string(), "invalid character 'b' looking for beginning of value");
        {
            let conn = store.lock().unwrap();
            conn.execute("UPDATE memories SET embedding=?", [&embedding]).unwrap();
            conn.execute("UPDATE memories SET metadata='[]',created_at='invalid' WHERE id='m-000'", []).unwrap();
        }
        assert!(store.search_ranked_prefiltered(&vector, "ollama", "global", 5).unwrap_err()
            .to_string().contains("column index 4, name \"created_at\""));
    }
}
