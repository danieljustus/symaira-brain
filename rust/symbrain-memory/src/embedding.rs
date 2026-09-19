//! The shipped hash-fallback embedding.
//!
//! When no embedding backend is reachable, the shipped implementation derives
//! a 768-dimensional vector from the text itself: lowercase, strip
//! punctuation, split into words, drop stop words, add `1` at the FNV-1a hash
//! of each remaining word modulo the dimension count, then L2-normalize in
//! `float32`. Reproducing it keeps rows written here in the same embedding
//! space as rows written by the shipped implementation, which is what makes
//! their search scores comparable.

/// Narrows a JSON number to the `float32` the store keeps.
#[allow(clippy::cast_possible_truncation)]
fn narrow(value: f64) -> f32 {
    value as f32
}

/// Dimension count of the shipped hash embedding.
pub(crate) const DIMENSIONS: usize = 768;

/// The shipped default Ollama endpoint (`config.Defaults().Ollama.URL`).
pub const DEFAULT_OLLAMA_URL: &str = "http://localhost:11434/api/embeddings";

/// The shipped default embedding model.
pub const DEFAULT_OLLAMA_MODEL: &str = "nomic-embed-text";

/// How long the shipped generator waits for Ollama before falling back.
const OLLAMA_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Embedding space name of the local hash fallback.
pub const HASH_FALLBACK_SOURCE: &str = "hash-fallback";

/// Embedding space name of vectors that came from Ollama.
pub const OLLAMA_SOURCE: &str = "ollama";

/// A query vector together with the embedding space it belongs to.
#[derive(Debug, Clone)]
pub struct GeneratedEmbedding {
    /// The 768-dimensional vector.
    pub vector: Vec<f32>,
    /// `ollama` or `hash-fallback`; retrieval only scores the same space.
    pub source: String,
}

/// Generates the query embedding the shipped order does: Ollama first, the
/// deterministic hash fallback when Ollama is unreachable or answers with a
/// vector of the wrong dimension.
///
/// The shipped generator also keeps a process-local vector cache and a 30s
/// failure cooldown; neither can be observed through a one-shot CLI call, so
/// they are deliberately not reproduced here.
pub struct EmbeddingGenerator {
    url: String,
    model: String,
}

impl Default for EmbeddingGenerator {
    fn default() -> Self {
        Self::new(DEFAULT_OLLAMA_URL, DEFAULT_OLLAMA_MODEL)
    }
}

impl EmbeddingGenerator {
    /// Builds a generator for a configured endpoint and model.
    #[must_use]
    pub fn new(url: &str, model: &str) -> Self {
        Self {
            url: if url.is_empty() {
                DEFAULT_OLLAMA_URL.to_owned()
            } else {
                url.to_owned()
            },
            model: if model.is_empty() {
                DEFAULT_OLLAMA_MODEL.to_owned()
            } else {
                model.to_owned()
            },
        }
    }

    /// Produces the embedding for `text`.
    #[must_use]
    pub fn generate(&self, text: &str) -> GeneratedEmbedding {
        match self.query_ollama(text) {
            Some(vector) if vector.len() == DIMENSIONS => GeneratedEmbedding {
                vector,
                source: OLLAMA_SOURCE.to_owned(),
            },
            _ => GeneratedEmbedding {
                vector: local_hash_vector(text),
                source: HASH_FALLBACK_SOURCE.to_owned(),
            },
        }
    }

    /// Posts the query to Ollama's OpenAI-compatible endpoint. Every failure —
    /// unreachable host, timeout, non-2xx, an unparseable body — is `None`, so
    /// the caller degrades to the hash fallback exactly like the shipped code.
    fn query_ollama(&self, text: &str) -> Option<Vec<f32>> {
        let endpoint = format!("{}/embeddings", self.base_url()?);
        let body = serde_json::json!({ "model": self.model, "input": [text] }).to_string();
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(OLLAMA_TIMEOUT))
            .max_redirects(0)
            .build();
        let agent = ureq::Agent::new_with_config(config);
        let request = ureq::http::Request::builder()
            .method("POST")
            .uri(endpoint.as_str())
            .header("content-type", "application/json")
            .body(body)
            .ok()?;
        let mut response = agent.run(request).ok()?;
        if !response.status().is_success() {
            return None;
        }
        let raw = response.body_mut().read_to_string().ok()?;
        let parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;
        let values = parsed
            .get("data")?
            .as_array()?
            .first()?
            .get("embedding")?
            .as_array()?;
        let vector: Vec<f32> = values
            .iter()
            .map(|value| value.as_f64().map(narrow))
            .collect::<Option<Vec<f32>>>()?;
        Some(vector)
    }

    /// Reduces the configured endpoint to its `scheme://host` root and appends
    /// the `/v1` prefix the shipped transport posts to.
    fn base_url(&self) -> Option<String> {
        let scheme_end = self.url.find("://")? + 3;
        let rest = &self.url[scheme_end..];
        let host_end = rest.find('/').unwrap_or(rest.len());
        let root = format!("{}{}", &self.url[..scheme_end], &rest[..host_end]);
        if root.ends_with("/v1") {
            Some(root)
        } else {
            Some(format!("{root}/v1"))
        }
    }
}

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
