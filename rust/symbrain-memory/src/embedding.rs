//! The shipped hash-fallback embedding.
//!
//! When no embedding backend is reachable, the shipped implementation derives
//! a 768-dimensional vector from the text itself: lowercase, strip
//! punctuation, split into words, drop stop words, add `1` at the FNV-1a hash
//! of each remaining word modulo the dimension count, then L2-normalize in
//! `float32`. Reproducing it keeps rows written here in the same embedding
//! space as rows written by the shipped implementation, which is what makes
//! their search scores comparable.

/// Dimension count of the shipped hash embedding.
pub(crate) const DIMENSIONS: usize = 768;

/// Compact English/German stop-word list, exactly as the shipped code has it.
const STOP_WORDS: [&str; 29] = [
    "and", "the", "a", "an", "of", "to", "in", "is", "it", "that", "und", "der", "die", "das",
    "ein", "eine", "ist", "es", "dass", "von", "zu", "mit", "auf", "für", "den", "dem", "des",
    "im", "am",
];

/// Characters the shipped implementation turns into word separators.
const PUNCTUATION: [char; 14] = [
    '.', ',', '!', '?', ';', ':', '-', '_', '(', ')', '[', ']', '{', '}',
];

/// Derives the hash-fallback vector for `text`.
#[must_use]
pub(crate) fn local_hash_vector(text: &str) -> Vec<f32> {
    let mut vector = vec![0.0_f32; DIMENSIONS];
    let mut cleaned = text.to_lowercase();
    for character in PUNCTUATION {
        cleaned = cleaned.replace(character, " ");
    }
    for word in cleaned.split_whitespace() {
        if STOP_WORDS.contains(&word) {
            continue;
        }
        let index = (fnv1a32(word) as usize) % DIMENSIONS;
        vector[index] += 1.0;
    }

    let sum_squares: f64 = vector.iter().map(|value| f64::from(*value * *value)).sum();
    if sum_squares > 0.0 {
        // Mirrors the shipped `float32(math.Sqrt(sumSquares))`: the narrowing
        // conversion is part of the contract, not an accident.
        #[allow(clippy::cast_possible_truncation)]
        let norm = sum_squares.sqrt() as f32;
        for value in &mut vector {
            *value /= norm;
        }
    }
    vector
}

/// FNV-1a, 32 bit — the hash function the shipped implementation uses.
fn fnv1a32(value: &str) -> u32 {
    let mut hash: u32 = 2_166_136_261;
    for byte in value.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::local_hash_vector;

    #[test]
    fn hash_fallback_is_normalized_and_deterministic() {
        let vector = local_hash_vector("Synchronisation der Speicher-Store");
        assert_eq!(vector.len(), 768);
        let norm: f64 = vector
            .iter()
            .map(|value| f64::from(*value * *value))
            .sum::<f64>()
            .sqrt();
        assert!((norm - 1.0).abs() < 1e-6, "norm {norm}");
        assert_eq!(
            vector,
            local_hash_vector("Synchronisation der Speicher-Store")
        );
    }

    #[test]
    fn stop_words_and_punctuation_do_not_change_the_direction() {
        // "the" is a stop word and punctuation becomes whitespace.
        assert_eq!(
            local_hash_vector("Memory: the store"),
            local_hash_vector("memory store")
        );
        // An all-stop-word text has no direction at all.
        let empty = local_hash_vector("the and of");
        assert!(empty.iter().all(|value| *value == 0.0));
    }
}
