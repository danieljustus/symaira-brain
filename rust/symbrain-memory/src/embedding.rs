//! The shipped hash-fallback embedding.
//!
//! When no embedding backend is reachable, the shipped implementation derives
//! a 768-dimensional vector from the text itself: lowercase, strip
//! punctuation, split into words, drop stop words, add `1` at the FNV-1a hash
//! of each remaining word modulo the dimension count, then L2-normalize in
//! `float32`. Reproducing it keeps rows written here in the same embedding
//! space as rows written by the shipped implementation, which is what makes
//! their search scores comparable.

use symaira_core_llm::{ClientBuilder, lookup};

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
        let descriptor = lookup("ollama")?;
        let client = ClientBuilder::new(descriptor.clone(), "")
            .base_url(self.base_url()?)
            .timeout(OLLAMA_TIMEOUT)
            .build()
            .ok()?;
        // CoreKit requires one response item per input. The Go llmkit transport
        // applies the same cardinality check before Brain reads the first item.
        let embeddings = client.embed(&self.model, &[text.to_owned()], None).ok()?;
        embeddings
            .into_iter()
            .next()
            .map(|embedding| embedding.vector)
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
    use super::{EmbeddingGenerator, HASH_FALLBACK_SOURCE, OLLAMA_SOURCE, local_hash_vector};
    use serde_json::Value;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;

    struct MockServer {
        url: String,
        join: Option<thread::JoinHandle<(String, Vec<u8>)>>,
    }

    impl MockServer {
        fn reply(status: &str, body: &str, delay: Duration) -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock server");
            let address = listener.local_addr().expect("mock server address");
            let status = status.to_owned();
            let body = body.as_bytes().to_vec();
            let join = thread::spawn(move || {
                let (stream, _) = listener.accept().expect("accept request");
                Self::serve(stream, &status, &body, delay)
            });
            Self {
                url: format!("http://{address}/api/embeddings"),
                join: Some(join),
            }
        }

        fn serve(
            mut stream: TcpStream,
            status: &str,
            body: &[u8],
            delay: Duration,
        ) -> (String, Vec<u8>) {
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
            let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
            let mut first_line = String::new();
            reader
                .read_line(&mut first_line)
                .expect("read request line");
            let mut content_length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("read request header");
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some(value) = line
                    .split_once(':')
                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .map(|(_, value)| value.trim())
                {
                    content_length = value.parse().expect("content length");
                }
            }
            let mut request_body = vec![0; content_length];
            reader
                .read_exact(&mut request_body)
                .expect("read request body");
            if !delay.is_zero() {
                thread::sleep(delay);
            }
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(body);
            (first_line, request_body)
        }

        fn request(&mut self) -> (String, Vec<u8>) {
            self.join
                .take()
                .expect("mock server joined")
                .join()
                .expect("mock server")
        }
    }

    fn embedding_response(count: usize, dimensions: usize) -> String {
        let vector = vec![0.25_f32; dimensions];
        let data = (0..count)
            .map(|_| serde_json::json!({"embedding": vector}))
            .collect::<Vec<_>>();
        serde_json::json!({"data": data}).to_string()
    }

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

    #[test]
    fn shared_llm_client_keeps_ollama_openai_request_contract() {
        let mut server = MockServer::reply("200 OK", &embedding_response(1, 768), Duration::ZERO);
        let result = EmbeddingGenerator::new(&server.url, "model-test").generate("query text");
        let (request_line, body) = server.request();
        let request: Value = serde_json::from_slice(&body).expect("request JSON");

        assert_eq!(result.source, OLLAMA_SOURCE);
        assert_eq!(result.vector, vec![0.25_f32; 768]);
        assert_eq!(request_line, "POST /v1/embeddings HTTP/1.1\r\n");
        assert_eq!(request["model"], "model-test");
        assert_eq!(request["input"], serde_json::json!(["query text"]));
        assert!(request.get("prompt").is_none());
    }

    #[test]
    fn shared_llm_client_rejects_extra_embedding_items_like_go_llmkit() {
        let mut server = MockServer::reply("200 OK", &embedding_response(2, 768), Duration::ZERO);
        let result = EmbeddingGenerator::new(&server.url, "model-test").generate("query text");
        let _ = server.request();

        assert_eq!(result.source, HASH_FALLBACK_SOURCE);
        assert_eq!(result.vector, local_hash_vector("query text"));
    }

    #[test]
    fn shared_llm_client_rejects_wrong_dimension_and_http_errors() {
        let mut wrong_dimensions =
            MockServer::reply("200 OK", &embedding_response(1, 767), Duration::ZERO);
        let wrong =
            EmbeddingGenerator::new(&wrong_dimensions.url, "model-test").generate("query text");
        let _ = wrong_dimensions.request();
        assert_eq!(wrong.source, HASH_FALLBACK_SOURCE);

        let mut error = MockServer::reply("503 Service Unavailable", "{}", Duration::ZERO);
        let failed = EmbeddingGenerator::new(&error.url, "model-test").generate("query text");
        let _ = error.request();
        assert_eq!(failed.source, HASH_FALLBACK_SOURCE);
    }

    #[test]
    fn shared_llm_client_times_out_to_hash_fallback() {
        let mut server = MockServer::reply(
            "200 OK",
            &embedding_response(1, 768),
            Duration::from_millis(2300),
        );
        let started = std::time::Instant::now();
        let result = EmbeddingGenerator::new(&server.url, "model-test").generate("query text");
        let elapsed = started.elapsed();
        let _ = server.request();

        assert_eq!(result.source, HASH_FALLBACK_SOURCE);
        assert!(elapsed < Duration::from_secs(3), "timeout took {elapsed:?}");
    }
}
