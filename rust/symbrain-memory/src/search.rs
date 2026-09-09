use chrono::Utc;
use rusqlite::params;

use super::{
    ActivityItem, ActivityPage, ActivitySearch, MAX_QUERY_LENGTH, MAX_RESULTS, MAX_TOKENS,
    Provenance,
};
use crate::{Store, StoreError};

pub(crate) fn search(store: &Store, opts: &ActivitySearch) -> Result<ActivityPage, StoreError> {
    validate(opts)?;
    let conn = store.activity_conn()?;
    let now = Utc::now().to_rfc3339();
    let query = opts.query.to_lowercase();
    let mut items = query_segments(&conn, opts, &now, &query)?;
    if opts.include_episodes {
        items.extend(query_episodes(&conn, opts, &now, &query)?);
    }
    Ok(build_page(items, opts.limit, opts.max_tokens))
}

fn validate(opts: &ActivitySearch) -> Result<(), StoreError> {
    if opts.query.trim().is_empty() {
        return Err(StoreError::Invalid("activity query is required".into()));
    }
    if opts.query.chars().count() > MAX_QUERY_LENGTH {
        return Err(StoreError::Invalid(format!(
            "activity query exceeds {MAX_QUERY_LENGTH} characters"
        )));
    }
    if opts.to <= opts.from {
        return Err(StoreError::Invalid(
            "activity query requires an increasing from/to window".into(),
        ));
    }
    if opts.to - opts.from > chrono::Duration::days(7) {
        return Err(StoreError::Invalid(
            "activity query window exceeds 168h0m0s".into(),
        ));
    }
    if !(1..=MAX_RESULTS).contains(&opts.limit) {
        return Err(StoreError::Invalid(format!(
            "activity query limit must be between 1 and {MAX_RESULTS}"
        )));
    }
    if !(1..=MAX_TOKENS).contains(&opts.max_tokens) {
        return Err(StoreError::Invalid(format!(
            "activity query max_tokens must be between 1 and {MAX_TOKENS}"
        )));
    }
    Ok(())
}

fn query_segments(
    conn: &rusqlite::Connection,
    opts: &ActivitySearch,
    now: &str,
    query: &str,
) -> Result<Vec<ActivityItem>, StoreError> {
    let mut stmt = conn.prepare("SELECT id,source,granularity,started_at,ended_at,applications,redacted_summary,raw_ref,prior_segment_ids,superseded_by FROM activity_segments WHERE ended_at > ? AND started_at < ? AND expires_at > ? AND (granularity != '10min' OR superseded_by = '') AND (? = '' OR source = ?) ORDER BY started_at ASC,id ASC")?;
    let rows = stmt.query_map(
        params![
            opts.from.to_rfc3339(),
            opts.to.to_rfc3339(),
            now,
            opts.source.as_str(),
            opts.source.as_str(),
        ],
        |row| {
            let applications: Vec<String> =
                serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or_default();
            let source: String = row.get(1)?;
            let summary: String = row.get(6)?;
            let matches = std::iter::once(source.as_str())
                .chain(std::iter::once(summary.as_str()))
                .chain(applications.iter().map(String::as_str))
                .any(|field| field.to_lowercase().contains(query));
            if !matches {
                return Ok(None);
            }
            Ok(Some(ActivityItem {
                id: row.get(0)?,
                kind: "segment".into(),
                source: source.clone(),
                granularity: row.get(2)?,
                started_at: super::parse_time(row.get(3)?)?,
                ended_at: super::parse_time(row.get(4)?)?,
                applications,
                summary,
                title: String::new(),
                scope: String::new(),
                confidence: 0.0,
                provenance: Provenance {
                    source,
                    reference: row.get(7)?,
                    prior_segment_ids: serde_json::from_str(&row.get::<_, String>(8)?)
                        .unwrap_or_default(),
                    derived_from: super::nonempty(row.get(9)?),
                    citations: Vec::new(),
                },
                tokens: 0,
            }))
        },
    )?;
    rows.filter_map(Result::transpose)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn query_episodes(
    conn: &rusqlite::Connection,
    opts: &ActivitySearch,
    now: &str,
    query: &str,
) -> Result<Vec<ActivityItem>, StoreError> {
    let mut stmt = conn.prepare("SELECT id,title,scope,started_at,ended_at,confidence,sources,citations FROM activity_episodes WHERE ended_at > ? AND started_at < ? AND expires_at > ? ORDER BY started_at ASC,id ASC")?;
    let rows = stmt.query_map(
        params![opts.from.to_rfc3339(), opts.to.to_rfc3339(), now],
        |row| {
            let title: String = row.get(1)?;
            let scope: String = row.get(2)?;
            if !title.to_lowercase().contains(query) && !scope.to_lowercase().contains(query) {
                return Ok(None);
            }
            Ok(Some(ActivityItem {
                id: row.get(0)?,
                kind: "episode".into(),
                title: title.clone(),
                scope,
                started_at: super::parse_time(row.get(3)?)?,
                ended_at: super::parse_time(row.get(4)?)?,
                confidence: row.get(5)?,
                summary: title,
                source: String::new(),
                granularity: String::new(),
                applications: Vec::new(),
                provenance: Provenance {
                    derived_from: serde_json::from_str(&row.get::<_, String>(6)?)
                        .unwrap_or_default(),
                    citations: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default(),
                    ..Provenance::default()
                },
                tokens: 0,
            }))
        },
    )?;
    rows.filter_map(Result::transpose)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn build_page(mut items: Vec<ActivityItem>, limit: usize, max_tokens: usize) -> ActivityPage {
    items.sort_by(|left, right| {
        left.started_at
            .cmp(&right.started_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let total = items.len();
    let mut page = ActivityPage {
        results: Vec::with_capacity(total.min(limit)),
        truncated: false,
        used_tokens: 0,
        max_tokens,
    };
    for mut item in items {
        if page.results.len() >= limit {
            page.truncated = true;
            break;
        }
        let remaining = max_tokens.saturating_sub(page.used_tokens);
        if remaining == 0 {
            page.truncated = true;
            break;
        }
        item.summary = item.summary.chars().take(remaining * 4).collect();
        item.tokens = super::tokens(&item.summary);
        page.used_tokens += item.tokens;
        page.results.push(item);
    }
    page.truncated |= page.results.len() < total;
    page
}
