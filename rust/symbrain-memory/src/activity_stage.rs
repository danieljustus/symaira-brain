//! Staging an activity episode as a memory candidate (ported for #761 from
//! `Store.StageEpisode` at v0.12.0).
//!
//! The candidate is never approved here: it is `review_status = 'staged'`,
//! carries `sync_exclude = "true"` so it never leaves this store, and gets one
//! grounded evidence span per source segment, all in one transaction.
//! Approval stays the explicit memory-promotion boundary. A lone 10min segment
//! is refused so a single window cannot become a durable profile fact.

use crate::activity_episode::{ActivityEpisode, episode_by_id};
use crate::activity_store::{GRANULARITY_10MIN, segment_by_id};
use crate::evidence::{Extraction, SourceRef, Span};
use crate::evidence_store::save_memory_evidence_tx;
use crate::gotime;
use crate::model::{Store, StoreError};
use crate::pii::redact;
use chrono::{DateTime, Utc};
use rusqlite::params;
use std::collections::BTreeMap;
use symbrain_activity::stable_id;

fn invalid(message: impl Into<String>) -> StoreError {
    StoreError::Invalid(message.into())
}

impl Store {
    /// Creates (or refreshes) the staged memory of `episode_id` and returns
    /// its id.
    ///
    /// # Errors
    /// A missing episode or segment, the lone-10min refusal, or a SQLite error.
    pub fn stage_activity_episode(
        &self,
        episode_id: &str,
        now: DateTime<Utc>,
    ) -> Result<String, StoreError> {
        let mut conn = self.lock()?;
        let episode = episode_by_id(&conn, episode_id)?
            .ok_or_else(|| invalid(format!("activity episode not found: {episode_id}")))?;
        let mut segments = Vec::new();
        for id in &episode.segment_ids {
            segments.push(
                segment_by_id(&conn, id)?
                    .ok_or_else(|| invalid(format!("activity segment not found: {id}")))?,
            );
        }
        if segments.len() == 1 && segments[0].granularity == GRANULARITY_10MIN {
            return Err(invalid(
                "single 10min activity segment cannot become a durable candidate",
            ));
        }
        let memory_id = stable_id(&["memory", "activity", &episode.id]);
        let transaction = conn.transaction()?;
        save_candidate(&transaction, &memory_id, &episode, now)?;
        // A restage replaces the earlier evidence instead of stacking it.
        transaction.execute(
            "DELETE FROM memory_evidence WHERE memory_id = ?",
            [&memory_id],
        )?;
        for segment in &segments {
            let extraction = Extraction {
                source: SourceRef {
                    id: segment.id.clone(),
                    kind: "activity_segment".to_owned(),
                },
                text: segment.redacted_summary.clone(),
                evidence_text: segment.redacted_summary.clone(),
                span: Span {
                    start: 0,
                    end: i64::try_from(segment.redacted_summary.len()).unwrap_or(i64::MAX),
                },
                alignment_status: "exact".to_owned(),
                attributes: BTreeMap::new(),
            };
            save_memory_evidence_tx(&transaction, &memory_id, &[extraction])?;
        }
        transaction.commit()?;
        Ok(memory_id)
    }
}

fn save_candidate(
    transaction: &rusqlite::Transaction<'_>,
    memory_id: &str,
    episode: &ActivityEpisode,
    now: DateTime<Utc>,
) -> Result<(), StoreError> {
    let content = redact(&episode.title);
    let scope = match redact(&episode.scope) {
        scope if scope.is_empty() => "activity".to_owned(),
        scope => scope,
    };
    let importance = if episode.confidence == 0.0 {
        0.5
    } else {
        episode.confidence
    };
    let metadata = serde_json::json!({
        "source": "activity",
        "activity_source": "true",
        "activity_episode_id": episode.id,
        "sync_exclude": "true",
        "promotion_boundary": "memory_candidates",
    })
    .to_string();
    transaction.execute(
        "INSERT INTO memories (id, content, scope, metadata, embedding, content_hash, created_at, updated_at, created_by, updated_by, created_session, updated_session, consolidation_status, importance, review_status, kind) VALUES (?, ?, ?, ?, '[]', ?, ?, ?, 'activity', 'activity', ?, ?, 'raw', ?, 'staged', 'reference') ON CONFLICT(id) DO UPDATE SET content=excluded.content, scope=excluded.scope, metadata=excluded.metadata, content_hash=excluded.content_hash, updated_at=excluded.updated_at, importance=excluded.importance",
        params![
            memory_id,
            content,
            scope,
            metadata,
            crate::write::content_hash(&content),
            gotime::format(episode.started_at),
            gotime::format(now),
            episode.id,
            episode.id,
            importance,
        ],
    )?;
    Ok(())
}
