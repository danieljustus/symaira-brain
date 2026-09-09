use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{Store, StoreError};

#[path = "search.rs"]
mod search;
pub(crate) use search::search;

pub const MAX_RESULTS: usize = 50;
pub const MAX_TOKENS: usize = 4000;
pub const MAX_QUERY_LENGTH: usize = 512;

#[derive(Debug, Clone, Deserialize)]
pub struct ActivitySearch {
    pub query: String,
    pub source: String,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub limit: usize,
    pub max_tokens: usize,
    #[serde(default)]
    pub include_episodes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityItem {
    pub id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub granularity: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub scope: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "is_zero")]
    pub confidence: f64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub applications: Vec<String>,
    pub summary: String,
    pub provenance: Provenance,
    pub tokens: usize,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Provenance {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reference: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prior_segment_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub derived_from: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub citations: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityPage {
    pub results: Vec<ActivityItem>,
    pub truncated: bool,
    pub used_tokens: usize,
    pub max_tokens: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActivityStatus {
    pub active_segments: usize,
    pub active_episodes: usize,
    pub segment_ttl: String,
    pub episode_ttl: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub earliest: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<DateTime<Utc>>,
}
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

#[allow(clippy::needless_pass_by_value)]
fn parse_time(raw: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&raw)
        .map(|v| v.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                raw.len(),
                rusqlite::types::Type::Text,
                Box::new(e),
            )
        })
}
fn nonempty(v: String) -> Vec<String> {
    if v.is_empty() { Vec::new() } else { vec![v] }
}
fn tokens(s: &str) -> usize {
    if s.is_empty() {
        0
    } else {
        s.chars().count() / 4 + 1
    }
}

impl Store {
    /// Reads one non-expired activity item.
    ///
    /// # Errors
    /// Returns an error when the SQLite query or timestamp decoding fails.
    pub fn activity_get(&self, id: &str) -> Result<Option<ActivityItem>, StoreError> {
        let conn = self.activity_conn()?;
        let now = Utc::now().to_rfc3339();
        let segment=conn.query_row("SELECT id,source,granularity,started_at,ended_at,applications,redacted_summary,raw_ref,prior_segment_ids,superseded_by FROM activity_segments WHERE id=? AND expires_at > ?",params![id,now],|r|{Ok(ActivityItem{id:r.get(0)?,kind:"segment".into(),source:r.get(1)?,granularity:r.get(2)?,started_at:parse_time(r.get(3)?)?,ended_at:parse_time(r.get(4)?)?,applications:serde_json::from_str(&r.get::<_,String>(5)?).unwrap_or_default(),summary:r.get(6)?,title:String::new(),scope:String::new(),confidence:0.0,provenance:Provenance{source:r.get(1)?,reference:r.get(7)?,prior_segment_ids:serde_json::from_str(&r.get::<_,String>(8)?).unwrap_or_default(),derived_from:nonempty(r.get(9)?),citations:Vec::new()},tokens:0})}).optional()?;
        if segment.is_some() {
            return Ok(segment);
        }
        conn.query_row("SELECT id,title,scope,started_at,ended_at,confidence,sources,citations FROM activity_episodes WHERE id=? AND expires_at > ?",params![id,now],|r|{let title:String=r.get(1)?;Ok(ActivityItem{id:r.get(0)?,kind:"episode".into(),title:title.clone(),scope:r.get(2)?,started_at:parse_time(r.get(3)?)?,ended_at:parse_time(r.get(4)?)?,confidence:r.get(5)?,summary:title,source:String::new(),granularity:String::new(),applications:Vec::new(),provenance:Provenance{derived_from:serde_json::from_str(&r.get::<_,String>(6)?).unwrap_or_default(),citations:serde_json::from_str(&r.get::<_,String>(7)?).unwrap_or_default(),..Provenance::default()},tokens:0})}).optional().map_err(Into::into)
    }
    /// Returns counts and the active activity time range.
    ///
    /// # Errors
    /// Returns an error when the SQLite query or timestamp decoding fails.
    pub fn activity_status(&self) -> Result<ActivityStatus, StoreError> {
        let conn = self.activity_conn()?;
        let now = Utc::now().to_rfc3339();
        let segments: i64 = conn.query_row(
            "SELECT COUNT(*) FROM activity_segments WHERE expires_at > ?",
            [&now],
            |r| r.get(0),
        )?;
        let episodes: i64 = conn.query_row(
            "SELECT COUNT(*) FROM activity_episodes WHERE expires_at > ?",
            [&now],
            |r| r.get(0),
        )?;
        let (earliest_raw, latest_raw): (Option<String>, Option<String>) = conn.query_row("SELECT MIN(started_at), MAX(ended_at) FROM (SELECT started_at, ended_at FROM activity_segments WHERE expires_at > ? UNION ALL SELECT started_at, ended_at FROM activity_episodes WHERE expires_at > ?)", params![&now, &now], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let earliest = earliest_raw.map(parse_time).transpose()?;
        let latest = latest_raw.map(parse_time).transpose()?;
        Ok(ActivityStatus {
            active_segments: usize::try_from(segments).unwrap_or(usize::MAX),
            active_episodes: usize::try_from(episodes).unwrap_or(usize::MAX),
            segment_ttl: "48h0m0s".into(),
            episode_ttl: "720h0m0s".into(),
            earliest,
            latest,
        })
    }
}
