//! Locality-sensitive hashing for vector search.
//!
//! The shipped retrieval path never scores the whole table: it expands the
//! query vector into adjacent LSH buckets and reads only those rows. The
//! projection vectors decide which memories are even considered, so both the
//! projections and the bucket order are ported exactly — see
//! [`crate::gorand`] for the value stream they come from.

use crate::embedding::DIMENSIONS;
use crate::gorand::lsh_projections;

/// Number of LSH hash bits, hence `2^16` buckets.
const LSH_BITS: usize = 16;

/// Computes the LSH hash of a vector: one bit per fixed random projection,
/// set when the projection's dot product is non-negative.
///
/// An empty vector hashes to `0`, and a vector whose length differs from the
/// embedding dimension is a space mismatch — the shipped code rejects it
/// rather than truncating silently.
pub(crate) fn compute_lsh(vector: &[f32]) -> Result<i64, String> {
    if vector.is_empty() {
        return Ok(0);
    }
    if vector.len() != DIMENSIONS {
        return Err(format!(
            "lsh: dimension mismatch: got {}, expected {DIMENSIONS}",
            vector.len()
        ));
    }
    let mut hash: i64 = 0;
    for (index, projection) in lsh_projections().iter().enumerate() {
        let mut dot = 0.0_f64;
        for (value, weight) in vector.iter().zip(projection.iter()) {
            // The shipped loop accumulates `float64(vec[j] * proj[j])`: the
            // product is rounded in `float32` before it widens.
            dot += f64::from(*value * *weight);
        }
        if dot >= 0.0 {
            hash |= 1_i64 << index;
        }
    }
    Ok(hash)
}

/// Returns every LSH hash within `max_distance` bit flips of `base`, itself
/// included, in the shipped depth-first order (bit kept before bit flipped).
pub(crate) fn lsh_neighbors(base: i64, max_distance: u32) -> Vec<i64> {
    if max_distance == 0 {
        return vec![base];
    }
    let mut neighbors = Vec::new();
    walk(0, 0, base, max_distance, &mut neighbors);
    neighbors
}

fn walk(index: usize, distance: u32, current: i64, max_distance: u32, out: &mut Vec<i64>) {
    if index == LSH_BITS {
        out.push(current);
        return;
    }
    walk(index + 1, distance, current, max_distance, out);
    if distance < max_distance {
        walk(
            index + 1,
            distance + 1,
            current ^ (1_i64 << index),
            max_distance,
            out,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{compute_lsh, lsh_neighbors};
    use crate::embedding::local_hash_vector;

    #[test]
    fn a_dimension_mismatch_is_rejected_instead_of_truncated() {
        assert_eq!(compute_lsh(&[]), Ok(0));
        assert!(compute_lsh(&[1.0; 3]).is_err());
        assert!(compute_lsh(&local_hash_vector("alpha")).is_ok());
    }

    #[test]
    fn the_hash_is_deterministic_and_covers_the_bucket_range() {
        let first = compute_lsh(&local_hash_vector("alpha")).unwrap();
        assert_eq!(first, compute_lsh(&local_hash_vector("alpha")).unwrap());
        assert!((0..1 << 16).contains(&first));
    }

    /// Pinned against the shipped binary: `symbrain memory set <text>` stores
    /// `ComputeLSH(GenerateLocalHashVector(<text>))`. The same three numbers
    /// are printed by `go run ./scripts/gorand-dump` (`lsh <hash>` lines).
    #[test]
    fn the_hash_matches_the_shipped_store() {
        for (content, expected) in [
            ("alpha memory content", 59_256_i64),
            ("beta note about rust parity", 49_355),
            ("Kosinus-Ranking für Suche", 2_645),
        ] {
            assert_eq!(
                compute_lsh(&local_hash_vector(content)).unwrap(),
                expected,
                "{content}"
            );
        }
    }

    #[test]
    fn neighbors_cover_every_bucket_within_two_flips() {
        // 16 bits, distances 0, 1 and 2: 1 + 16 + 120.
        let neighbors = lsh_neighbors(0b1010_1010, 2);
        assert_eq!(neighbors.len(), 1 + 16 + 120);
        assert_eq!(neighbors[0], 0b1010_1010);
        assert_eq!(lsh_neighbors(7, 0), vec![7]);
        let unique: std::collections::HashSet<_> = neighbors.iter().collect();
        assert_eq!(unique.len(), neighbors.len());
        for neighbor in &neighbors {
            assert!((neighbor ^ 0b1010_1010).count_ones() <= 2);
        }
    }
}
