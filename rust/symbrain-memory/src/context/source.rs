//! Reads and writes the assembler needs from the store.

use super::Recalled;
use crate::gotime;
use crate::model::{Store, StoreError};
use chrono::{DateTime, Utc};
use rusqlite::params;

/// The store operations behind context assembly.
pub trait ContextSource {
    /// `GetSessionSummary`; empty when there is none.
    ///
    /// # Errors
    /// A store failure.
    fn session_summary(&self, id: &str) -> Result<String, StoreError>;

    /// `SaveSessionSummary` (insert or update).
    ///
    /// # Errors
    /// A store failure.
    fn save_session_summary(&self, id: &str, summary: &str) -> Result<(), StoreError>;

    /// `GetWorkingMemories`: live, non-archived working-tier memories,
    /// newest first.
    ///
    /// # Errors
    /// A store failure.
    fn working_memories(&self, limit: usize) -> Result<Vec<Recalled>, StoreError>;

    /// `SearchMemoriesBM25` over all scopes.
    ///
    /// # Errors
    /// A store failure.
    fn keyword_search(&self, query: &str, limit: usize) -> Result<Vec<Recalled>, StoreError>;

    /// `SearchMemories`: nearest memories of one embedding space.
    ///
    /// # Errors
    /// A store failure.
    fn vector_search(
        &self,
        vector: &[f32],
        source: &str,
        limit: usize,
    ) -> Result<Vec<Recalled>, StoreError>;
}

/// [`ContextSource`] over a [`Store`] with an injected clock.
pub struct StoreSource<'a> {
    store: &'a Store,
    now: &'a dyn Fn() -> DateTime<Utc>,
}

impl<'a> StoreSource<'a> {
    /// A source over `store`; `now` stamps summaries and filters expiry.
    #[must_use]
    pub fn new(store: &'a Store, now: &'a dyn Fn() -> DateTime<Utc>) -> Self {
        Self { store, now }
    }
}

/// Go's `Tokenize`: lowercase letters/digits, no stop words, no 1-byte words.
pub(crate) fn tokenize(text: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "a", "an", "the", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
        "do", "does", "did", "will", "would", "could", "should", "may", "might", "shall", "can",
        "to", "of", "in", "for", "on", "with", "at", "by", "from", "as", "into", "through",
        "during", "before", "after", "above", "below", "between", "out", "off", "over", "under",
        "again", "further", "then", "once", "and", "but", "or", "nor", "not", "so", "yet", "both",
        "either", "neither", "each", "every", "all", "any", "few", "more", "most", "other", "some",
        "such", "no", "only", "own", "same", "than", "too", "very", "just", "because", "if",
        "when", "where", "how", "what", "which", "who", "whom", "this", "that", "these", "those",
        "i", "me", "my", "we", "our", "you", "your", "he", "him", "his", "she", "her", "it", "its",
        "they", "them", "their",
    ];
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut flush = |current: &mut String| {
        if !current.is_empty() {
            if !STOP.contains(&current.as_str()) && current.len() > 1 {
                tokens.push(std::mem::take(current));
            } else {
                current.clear();
            }
        }
    };
    for ch in text.to_lowercase().chars() {
        if ch.is_alphanumeric() {
            current.push(ch);
        } else {
            flush(&mut current);
        }
    }
    flush(&mut current);
    tokens
}

fn recalled(row: &rusqlite::Row<'_>, score: f32) -> rusqlite::Result<Recalled> {
    let created: String = row.get(5)?;
    Ok(Recalled {
        id: row.get(0)?,
        content: row.get(1)?,
        scope: row.get(2)?,
        kind: row.get(3)?,
        tier: row.get(4)?,
        created_at: gotime::parse(&created).unwrap_or_default(),
        score,
    })
}

#[allow(clippy::needless_pass_by_value)] // used as `map_err(wrap)`
fn wrap(error: rusqlite::Error) -> StoreError {
    StoreError::Invalid(error.to_string())
}

impl ContextSource for StoreSource<'_> {
    fn session_summary(&self, id: &str) -> Result<String, StoreError> {
        let conn = self.store.lock()?;
        let mut statement = conn
            .prepare("SELECT summary FROM sessions WHERE id = ?")
            .map_err(wrap)?;
        let mut rows = statement.query([id]).map_err(wrap)?;
        match rows.next().map_err(wrap)? {
            Some(row) => row.get(0).map_err(wrap),
            None => Ok(String::new()),
        }
    }

    fn save_session_summary(&self, id: &str, summary: &str) -> Result<(), StoreError> {
        let now = gotime::format((self.now)());
        let conn = self.store.lock()?;
        conn.execute(
            "INSERT INTO sessions (id, summary, updated_at) VALUES (?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET summary=excluded.summary, updated_at=excluded.updated_at",
            params![id, summary, now],
        )
        .map_err(wrap)?;
        Ok(())
    }

    fn working_memories(&self, limit: usize) -> Result<Vec<Recalled>, StoreError> {
        let now = gotime::format((self.now)());
        let conn = self.store.lock()?;
        let mut statement = conn
            .prepare(
                "SELECT id, content, scope, kind, tier, created_at FROM memories \
                 WHERE tier = 'working' AND (expires_at IS NULL OR symbrain_sync_time(expires_at) > symbrain_sync_time(?)) \
                 AND consolidation_status != 'archived' ORDER BY created_at DESC LIMIT ?",
            )
            .map_err(wrap)?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = statement
            .query_map(params![now, limit], |row| recalled(row, 0.0))
            .map_err(wrap)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(wrap)
    }

    fn keyword_search(&self, query: &str, limit: usize) -> Result<Vec<Recalled>, StoreError> {
        let terms = tokenize(query);
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.store.lock()?;
        let mut statement = conn
            .prepare(
                "SELECT m.id, m.content, m.scope, m.kind, m.tier, m.created_at, rank \
                 FROM memories_fts fts JOIN memories m ON fts.id = m.id \
                 WHERE memories_fts MATCH ? AND m.review_status = 'approved' AND m.retired_at IS NULL \
                 ORDER BY rank LIMIT ?",
            )
            .map_err(wrap)?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = statement
            .query_map(params![terms.join(" OR "), limit], |row| {
                let rank: f64 = row.get(6)?;
                #[allow(clippy::cast_possible_truncation)]
                recalled(row, (-rank) as f32)
            })
            .map_err(wrap)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(wrap)
    }

    fn vector_search(
        &self,
        vector: &[f32],
        source: &str,
        limit: usize,
    ) -> Result<Vec<Recalled>, StoreError> {
        Ok(self
            .store
            .search_ranked(vector, source, "", limit)?
            .into_iter()
            .map(|hit| Recalled {
                id: hit.memory.id,
                content: hit.memory.content,
                scope: hit.memory.scope,
                kind: hit.memory.kind,
                tier: hit.memory.tier,
                created_at: hit
                    .memory
                    .created_at
                    .map(|t| t.with_timezone(&Utc))
                    .unwrap_or_default(),
                score: hit.score,
            })
            .collect())
    }
}
