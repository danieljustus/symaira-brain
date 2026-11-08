//! The DB-backed activity store (ported for #761 from
//! `internal/memory/activity` at v0.12.0): redacted fixed-window segments,
//! longer-lived episodes, the deterministic 6h rollup and episode staging.
//!
//! The read side (`activity_get`, `activity_status`, search) lives in
//! [`crate::activity`]; retention and clearing in `activity_retention`. The
//! store owns no capture code and never persists raw event payloads. Every
//! operation takes an injected `now` so results are deterministic.

use crate::gojson::timestamp;
use crate::gotime;
use crate::model::{Store, StoreError};
use crate::pii::redact;
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, Row, params};
use symbrain_activity::{stable_id, unique_sorted};

/// The fine-grained activity window.
pub const GRANULARITY_10MIN: &str = "10min";
/// The coarse rollup window.
pub const GRANULARITY_6H: &str = "6h";

/// Retention windows (`activity.Options`); non-positive values use defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityOptions {
    pub segment_ttl: Duration,
    pub episode_ttl: Duration,
}

impl Default for ActivityOptions {
    fn default() -> Self {
        Self {
            segment_ttl: Duration::hours(48),
            episode_ttl: Duration::hours(30 * 24),
        }
    }
}

/// A redacted, fixed-window observation (`activity.Segment`).
#[derive(Debug, Clone, PartialEq)]
pub struct ActivitySegment {
    pub id: String,
    pub source: String,
    pub granularity: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub applications: Vec<String>,
    pub redacted_summary: String,
    pub raw_ref: String,
    pub prior_segment_ids: Vec<String>,
    pub superseded_by: String,
    /// `None` takes `ended_at + segment_ttl`.
    pub expires_at: Option<DateTime<Utc>>,
}

pub(crate) const SEGMENT_COLUMNS: &str = "id, source, granularity, started_at, ended_at, applications, redacted_summary, raw_ref, prior_segment_ids, superseded_by, expires_at";
pub(crate) fn invalid(message: impl Into<String>) -> StoreError {
    StoreError::Invalid(message.into())
}

pub(crate) fn rfc3339(time: DateTime<Utc>) -> String {
    timestamp(Some(time)).unwrap_or_default()
}

pub(crate) fn parse(raw: &str) -> Result<DateTime<Utc>, StoreError> {
    gotime::parse(raw).ok_or_else(|| invalid(format!("unreadable activity timestamp {raw:?}")))
}

pub(crate) fn strings(raw: &str) -> Result<Vec<String>, StoreError> {
    serde_json::from_str(raw).map_err(|error| invalid(format!("activity list: {error}")))
}

pub(crate) fn json(values: &[String]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| "[]".to_owned())
}

impl ActivityOptions {
    pub(crate) fn normalized(self) -> Self {
        let defaults = Self::default();
        Self {
            segment_ttl: if self.segment_ttl > Duration::zero() {
                self.segment_ttl
            } else {
                defaults.segment_ttl
            },
            episode_ttl: if self.episode_ttl > Duration::zero() {
                self.episode_ttl
            } else {
                defaults.episode_ttl
            },
        }
    }
}

impl ActivitySegment {
    fn prepared(mut self, options: ActivityOptions) -> Self {
        self.source = redact(self.source.trim());
        self.redacted_summary = redact(&self.redacted_summary);
        self.raw_ref = redact(&self.raw_ref);
        self.applications = self.applications.iter().map(|a| redact(a)).collect();
        self.prior_segment_ids = unique_sorted(&self.prior_segment_ids);
        if self.id.is_empty() {
            self.id = segment_id(
                &self.source,
                &self.granularity,
                self.started_at,
                self.ended_at,
            );
        }
        self.expires_at
            .get_or_insert(self.ended_at + options.normalized().segment_ttl);
        self
    }

    fn validate(&self) -> Result<(), StoreError> {
        if self.id.is_empty() {
            return Err(invalid("activity segment id is required"));
        }
        if self.source.trim().is_empty() {
            return Err(invalid("activity segment source is required"));
        }
        if self.granularity != GRANULARITY_10MIN && self.granularity != GRANULARITY_6H {
            return Err(invalid(format!(
                "invalid activity segment granularity {:?}",
                self.granularity
            )));
        }
        if self.ended_at <= self.started_at {
            return Err(invalid(
                "activity segment requires an increasing time window",
            ));
        }
        if self.redacted_summary.is_empty() {
            return Err(invalid("activity segment summary is required"));
        }
        Ok(())
    }

    fn scan(row: &Row<'_>) -> Result<Self, StoreError> {
        let text = |index: usize| row.get::<_, String>(index).map_err(StoreError::from);
        Ok(Self {
            id: text(0)?,
            source: text(1)?,
            granularity: text(2)?,
            started_at: parse(&text(3)?)?,
            ended_at: parse(&text(4)?)?,
            applications: strings(&text(5)?)?,
            redacted_summary: text(6)?,
            raw_ref: text(7)?,
            prior_segment_ids: strings(&text(8)?)?,
            superseded_by: text(9)?,
            expires_at: Some(parse(&text(10)?)?),
        })
    }
}

/// `segmentID`: a stable id over source, granularity and the RFC 3339 window.
#[must_use]
pub fn segment_id(
    source: &str,
    granularity: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> String {
    stable_id(&[
        "segment",
        source,
        granularity,
        &rfc3339(start),
        &rfc3339(end),
    ])
}

fn save_segment(conn: &Connection, segment: &ActivitySegment) -> Result<(), StoreError> {
    let expires = segment
        .expires_at
        .ok_or_else(|| invalid("activity segment expiry is required"))?;
    conn.execute(
        "INSERT INTO activity_segments (id, source, granularity, started_at, ended_at, applications, redacted_summary, raw_ref, prior_segment_ids, superseded_by, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET source=excluded.source, granularity=excluded.granularity, started_at=excluded.started_at, ended_at=excluded.ended_at, applications=excluded.applications, redacted_summary=excluded.redacted_summary, raw_ref=excluded.raw_ref, prior_segment_ids=excluded.prior_segment_ids, superseded_by=excluded.superseded_by, expires_at=excluded.expires_at",
        params![
            segment.id,
            segment.source,
            segment.granularity,
            gotime::format(segment.started_at),
            gotime::format(segment.ended_at),
            json(&segment.applications),
            segment.redacted_summary,
            segment.raw_ref,
            json(&segment.prior_segment_ids),
            segment.superseded_by,
            gotime::format(expires),
        ],
    )?;
    Ok(())
}

impl Store {
    /// `SaveSegment`: redacts and upserts one segment.
    ///
    /// # Errors
    /// A validation or SQLite error.
    pub fn save_activity_segment(
        &self,
        segment: ActivitySegment,
        options: ActivityOptions,
    ) -> Result<ActivitySegment, StoreError> {
        let segment = segment.prepared(options);
        segment.validate()?;
        save_segment(&*self.lock()?, &segment)?;
        Ok(segment)
    }

    /// `GetSegment` without touching retention state.
    ///
    /// # Errors
    /// A SQLite or decoding error.
    pub fn activity_segment(&self, id: &str) -> Result<Option<ActivitySegment>, StoreError> {
        let conn = self.lock()?;
        segment_by_id(&conn, id)
    }

    /// `ListSegments` / `ListAllSegments`: non-expired segments overlapping
    /// `[start, end)`; fine rows covered by a rollup are omitted unless
    /// `include_superseded`.
    ///
    /// # Errors
    /// A SQLite or decoding error.
    pub fn list_activity_segments(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        now: DateTime<Utc>,
        include_superseded: bool,
    ) -> Result<Vec<ActivitySegment>, StoreError> {
        let conn = self.lock()?;
        let filter = if include_superseded {
            ""
        } else {
            " AND (granularity != '10min' OR superseded_by = '')"
        };
        let mut statement = conn.prepare(&format!(
            "SELECT {SEGMENT_COLUMNS} FROM activity_segments WHERE ended_at > ? AND started_at < ? AND expires_at > ?{filter} ORDER BY started_at ASC, id ASC"
        ))?;
        let mut rows = statement.query(params![
            gotime::format(start),
            gotime::format(end),
            gotime::format(now)
        ])?;
        let mut segments = Vec::new();
        while let Some(row) = rows.next()? {
            segments.push(ActivitySegment::scan(row)?);
        }
        Ok(segments)
    }

    /// `RollUp6Hour`: creates or replaces the deterministic 6h segment for
    /// `source` and `start`, records every consulted 10min segment and marks
    /// those rows superseded. Re-running it is idempotent.
    ///
    /// # Errors
    /// No active 10min segment in the window, or a SQLite error.
    pub fn rollup_activity_6h(
        &self,
        source: &str,
        start: DateTime<Utc>,
        options: ActivityOptions,
        now: DateTime<Utc>,
    ) -> Result<ActivitySegment, StoreError> {
        let source = redact(source.trim());
        let end = start + Duration::hours(6);
        let conn = self.lock()?;
        let fine = {
            let mut statement = conn.prepare(&format!(
                "SELECT {SEGMENT_COLUMNS} FROM activity_segments WHERE source = ? AND granularity = '10min' AND started_at >= ? AND ended_at <= ? AND expires_at > ? ORDER BY started_at ASC, id ASC"
            ))?;
            let mut rows = statement.query(params![
                source,
                gotime::format(start),
                gotime::format(end),
                gotime::format(now)
            ])?;
            let mut fine = Vec::new();
            while let Some(row) = rows.next()? {
                fine.push(ActivitySegment::scan(row)?);
            }
            fine
        };
        if fine.is_empty() {
            return Err(invalid(format!(
                "no active 10min segments in {} activity window",
                rfc3339(start)
            )));
        }
        let mut summaries = Vec::new();
        let (mut applications, mut prior) = (Vec::new(), Vec::new());
        for segment in &fine {
            summaries.push(segment.redacted_summary.clone());
            applications.extend(segment.applications.iter().cloned());
            prior.push(segment.id.clone());
            prior.extend(segment.prior_segment_ids.iter().cloned());
        }
        let rollup = ActivitySegment {
            id: segment_id(&source, GRANULARITY_6H, start, end),
            source: source.clone(),
            granularity: GRANULARITY_6H.to_owned(),
            started_at: start,
            ended_at: end,
            applications: unique_sorted(&applications),
            redacted_summary: redact(&summaries.join("\n")),
            raw_ref: format!("rollup:{source}:{}", start.format("%Y-%m-%dT%H:%M:%SZ")),
            prior_segment_ids: unique_sorted(&prior),
            superseded_by: String::new(),
            expires_at: Some(end + options.normalized().segment_ttl),
        }
        .prepared(options);
        rollup.validate()?;
        let transaction = conn.unchecked_transaction()?;
        save_segment(&transaction, &rollup)?;
        for segment in &fine {
            transaction.execute(
                "UPDATE activity_segments SET superseded_by = ? WHERE id = ?",
                params![rollup.id, segment.id],
            )?;
        }
        transaction.commit()?;
        Ok(rollup)
    }
}

pub(crate) fn segment_by_id(
    conn: &Connection,
    id: &str,
) -> Result<Option<ActivitySegment>, StoreError> {
    let mut statement = conn.prepare(&format!(
        "SELECT {SEGMENT_COLUMNS} FROM activity_segments WHERE id = ?"
    ))?;
    let mut rows = statement.query([id])?;
    rows.next()?.map(ActivitySegment::scan).transpose()
}
