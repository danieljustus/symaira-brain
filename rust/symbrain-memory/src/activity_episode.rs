//! Activity episodes (ported for #761 from `internal/memory/activity` at
//! v0.12.0): longer-lived rollups assembled from segment ids. Sources and
//! citations are references, never raw event content.

use crate::activity_store::{ActivityOptions, invalid, json, parse, rfc3339, strings};
use crate::gotime;
use crate::model::{Store, StoreError};
use crate::pii::redact;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, Row, params};
use symbrain_activity::{stable_id, unique_sorted};

/// A longer-lived rollup assembled from segment ids (`activity.Episode`).
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityEpisode {
    pub id: String,
    pub title: String,
    pub scope: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub confidence: f64,
    pub segment_ids: Vec<String>,
    pub citations: Vec<String>,
    /// `None` takes `ended_at + episode_ttl`.
    pub expires_at: Option<DateTime<Utc>>,
}

pub(crate) const EPISODE_COLUMNS: &str =
    "id, title, scope, started_at, ended_at, confidence, sources, citations, expires_at";

impl ActivityEpisode {
    fn prepared(mut self, options: ActivityOptions) -> Self {
        self.title = redact(self.title.trim());
        self.scope = redact(self.scope.trim());
        self.segment_ids = unique_sorted(&self.segment_ids);
        let citations: Vec<String> = self.citations.iter().map(|c| redact(c)).collect();
        self.citations = unique_sorted(&citations);
        if self.id.is_empty() {
            self.id = stable_id(&[
                "episode",
                &self.scope,
                &rfc3339(self.started_at),
                &rfc3339(self.ended_at),
            ]);
        }
        self.expires_at
            .get_or_insert(self.ended_at + options.normalized().episode_ttl);
        self
    }

    fn validate(&self) -> Result<(), StoreError> {
        if self.id.is_empty() {
            return Err(invalid("activity episode id is required"));
        }
        if self.title.is_empty() {
            return Err(invalid("activity episode title is required"));
        }
        if self.ended_at <= self.started_at {
            return Err(invalid(
                "activity episode requires an increasing time window",
            ));
        }
        if !(0.0..=1.0).contains(&self.confidence) {
            return Err(invalid(
                "activity episode confidence must be between 0 and 1",
            ));
        }
        Ok(())
    }

    fn scan(row: &Row<'_>) -> Result<Self, StoreError> {
        let text = |index: usize| row.get::<_, String>(index).map_err(StoreError::from);
        Ok(Self {
            id: text(0)?,
            title: text(1)?,
            scope: text(2)?,
            started_at: parse(&text(3)?)?,
            ended_at: parse(&text(4)?)?,
            confidence: row.get(5)?,
            segment_ids: strings(&text(6)?)?,
            citations: strings(&text(7)?)?,
            expires_at: Some(parse(&text(8)?)?),
        })
    }
}

impl Store {
    /// `SaveEpisode`: redacts and upserts one episode.
    ///
    /// # Errors
    /// A validation or SQLite error.
    pub fn save_activity_episode(
        &self,
        episode: ActivityEpisode,
        options: ActivityOptions,
    ) -> Result<ActivityEpisode, StoreError> {
        let episode = episode.prepared(options);
        episode.validate()?;
        let expires = episode
            .expires_at
            .ok_or_else(|| invalid("activity episode expiry is required"))?;
        self.lock()?.execute(
            "INSERT INTO activity_episodes (id, title, scope, started_at, ended_at, confidence, sources, citations, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET title=excluded.title, scope=excluded.scope, started_at=excluded.started_at, ended_at=excluded.ended_at, confidence=excluded.confidence, sources=excluded.sources, citations=excluded.citations, expires_at=excluded.expires_at",
            params![
                episode.id,
                episode.title,
                episode.scope,
                gotime::format(episode.started_at),
                gotime::format(episode.ended_at),
                episode.confidence,
                json(&episode.segment_ids),
                json(&episode.citations),
                gotime::format(expires),
            ],
        )?;
        Ok(episode)
    }

    /// `GetEpisode`: `None` when it does not exist.
    ///
    /// # Errors
    /// A SQLite or decoding error.
    pub fn activity_episode(&self, id: &str) -> Result<Option<ActivityEpisode>, StoreError> {
        let conn = self.lock()?;
        episode_by_id(&conn, id)
    }

    /// `ListEpisodes`: non-expired episodes overlapping `[start, end)`.
    ///
    /// # Errors
    /// A SQLite or decoding error.
    pub fn list_activity_episodes(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<Vec<ActivityEpisode>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn.prepare(&format!(
            "SELECT {EPISODE_COLUMNS} FROM activity_episodes WHERE ended_at > ? AND started_at < ? AND expires_at > ? ORDER BY started_at ASC, id ASC"
        ))?;
        let mut rows = statement.query(params![
            gotime::format(start),
            gotime::format(end),
            gotime::format(now)
        ])?;
        let mut episodes = Vec::new();
        while let Some(row) = rows.next()? {
            episodes.push(ActivityEpisode::scan(row)?);
        }
        Ok(episodes)
    }
}

pub(crate) fn episode_by_id(
    conn: &Connection,
    id: &str,
) -> Result<Option<ActivityEpisode>, StoreError> {
    let mut statement = conn.prepare(&format!(
        "SELECT {EPISODE_COLUMNS} FROM activity_episodes WHERE id = ?"
    ))?;
    let mut rows = statement.query([id])?;
    rows.next()?.map(ActivityEpisode::scan).transpose()
}
