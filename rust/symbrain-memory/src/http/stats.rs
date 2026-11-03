//! Live counters behind `/api/stats` and the embedding fallback state that
//! decides `/api/status`'s `embedding_backend`.

use super::Server;
use crate::GeneratedEmbedding;
use crate::gojson::number;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The shipped Ollama failure cooldown (`ollamaCacheTTL`).
const COOLDOWN: Duration = Duration::from_secs(30);

#[derive(Default)]
struct Retrieval {
    total: i64,
    zero: i64,
    min: f64,
    max: f64,
    sum: f64,
    scored: i64,
    latency_ns: i128,
}

#[derive(Default)]
struct Embedding {
    total: i64,
    hits: i64,
    misses: i64,
    fallbacks: i64,
    latency_ns: i128,
    last_fail: Option<Instant>,
}

#[derive(Default)]
pub(super) struct Metrics {
    retrieval: Mutex<Retrieval>,
    embedding: Mutex<Embedding>,
}

impl Metrics {
    /// `RetrievalStats.Record`.
    pub fn record(&self, results: usize, top_score: f64, latency: Duration) {
        let Ok(mut stats) = self.retrieval.lock() else {
            return;
        };
        stats.total += 1;
        stats.latency_ns += i128::try_from(latency.as_nanos()).unwrap_or(0);
        if results == 0 {
            stats.zero += 1;
            return;
        }
        if stats.scored == 0 {
            stats.min = top_score;
            stats.max = top_score;
        } else {
            stats.min = stats.min.min(top_score);
            stats.max = stats.max.max(top_score);
        }
        stats.sum += top_score;
        stats.scored += 1;
    }

    /// `ActiveBackend`: `lexical` inside the failure cooldown.
    pub fn backend(&self) -> &'static str {
        match self.embedding.lock() {
            Ok(state) if state.last_fail.is_some_and(|at| at.elapsed() < COOLDOWN) => "lexical",
            _ => "ollama",
        }
    }

    /// The `SystemStats` document, in the shipped field order.
    pub fn document(&self) -> String {
        #[allow(clippy::cast_precision_loss)]
        let (retrieval, embedding) = {
            let stats = self.retrieval.lock().map(|s| {
                let average = if s.scored > 0 {
                    s.sum / s.scored as f64
                } else {
                    0.0
                };
                let latency = if s.total > 0 {
                    s.latency_ns as f64 / s.total as f64 / 1e6
                } else {
                    0.0
                };
                format!(
                    "{{\"total_queries\":{},\"zero_result_queries\":{},\"score_min\":{},\"score_max\":{},\"score_avg\":{},\"avg_latency_ms\":{}}}",
                    s.total,
                    s.zero,
                    number(s.min),
                    number(s.max),
                    number(average),
                    number(latency)
                )
            });
            let embed = self.embedding.lock().map(|e| {
                let requests = e.hits + e.misses;
                let latency = if requests > 0 {
                    e.latency_ns as f64 / requests as f64 / 1e6
                } else {
                    0.0
                };
                let rate = if e.total > 0 {
                    e.fallbacks as f64 / e.total as f64
                } else {
                    0.0
                };
                format!(
                    "{{\"TotalRequests\":{},\"OllamaHits\":{},\"OllamaMisses\":{},\"FallbackCount\":{},\"FallbackRate\":{},\"AvgOllamaMs\":{}}}",
                    e.total,
                    e.hits,
                    e.misses,
                    e.fallbacks,
                    number(rate),
                    number(latency)
                )
            });
            (stats.unwrap_or_default(), embed.unwrap_or_default())
        };
        // One serialized SQLite connection: the pool is permanently one wide.
        format!(
            "{{\"retrieval\":{retrieval},\"embedding\":{embedding},\"database\":{{\"MaxOpenConnections\":1,\"OpenConnections\":1,\"InUse\":0,\"Idle\":1,\"WaitCount\":0,\"WaitDuration\":0,\"MaxIdleClosed\":0,\"MaxIdleTimeClosed\":0,\"MaxLifetimeClosed\":0}}}}"
        )
    }
}

impl Server {
    /// `GenerateVector`: Ollama unless inside the failure cooldown, then the
    /// deterministic hash fallback. The vector cache is not reproduced.
    pub(super) fn embed(&self, text: &str) -> GeneratedEmbedding {
        let skip = {
            let Ok(mut state) = self.metrics.embedding.lock() else {
                return self.generator.generate(text);
            };
            state.total += 1;
            state.last_fail.is_some_and(|at| at.elapsed() < COOLDOWN)
        };
        if !skip {
            let start = Instant::now();
            let vector = self.generator.try_ollama(text);
            let elapsed = start.elapsed();
            if let Ok(mut state) = self.metrics.embedding.lock() {
                state.latency_ns += i128::try_from(elapsed.as_nanos()).unwrap_or(0);
                if vector.is_some() {
                    state.hits += 1;
                } else {
                    state.misses += 1;
                    state.last_fail = Some(Instant::now());
                }
            }
            if let Some(vector) = vector {
                return GeneratedEmbedding {
                    vector,
                    source: crate::embedding::OLLAMA_SOURCE.to_owned(),
                };
            }
        }
        if let Ok(mut state) = self.metrics.embedding.lock() {
            state.fallbacks += 1;
        }
        GeneratedEmbedding {
            vector: crate::embedding::local_hash_vector(text),
            source: crate::embedding::HASH_FALLBACK_SOURCE.to_owned(),
        }
    }
}
