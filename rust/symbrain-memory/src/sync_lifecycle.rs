//! Deterministic reconciliation of lifecycle edits made on two peers at the
//! same `updated_at` (#855).
//!
//! Last-writer-wins on `updated_at` decides every other case. When two peers
//! stamp the same instant but disagree on retirement, supersession,
//! consolidation or validity, each peer ranks the two states by the same
//! canonical key and keeps the greater one, so both converge without a
//! coordinator. The change is written to the audit log as `sync_lifecycle`
//! with the losing and winning keys.

use crate::cli_write::audit;
use crate::sync_time::normalize_sync_time;
use rusqlite::{Connection, OptionalExtension, params};

/// The lifecycle columns of one memory in the writer's own timestamp spelling.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Lifecycle {
    pub retired_at: Option<String>,
    pub superseded_by: String,
    pub consolidated_into_id: String,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
}

fn normalized(value: Option<&str>) -> String {
    value
        .filter(|text| !text.is_empty())
        .map(|text| normalize_sync_time(text).unwrap_or_else(|_| text.to_owned()))
        .unwrap_or_default()
}

impl Lifecycle {
    /// The canonical ranking key; greater wins. Retirement outranks everything
    /// (a retired memory never silently comes back), then supersession,
    /// consolidation and validity.
    pub(crate) fn key(&self) -> String {
        [
            normalized(self.retired_at.as_deref()),
            self.superseded_by.clone(),
            self.consolidated_into_id.clone(),
            normalized(self.valid_to.as_deref()),
            normalized(self.valid_from.as_deref()),
        ]
        .join("\u{1f}")
    }

    fn stored(conn: &Connection, id: &str) -> rusqlite::Result<Option<Self>> {
        conn.query_row(
            "SELECT retired_at, superseded_by, consolidated_into_id, valid_from, valid_to FROM memories WHERE id=?",
            [id],
            |row| {
                Ok(Self {
                    retired_at: row.get(0)?,
                    superseded_by: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    consolidated_into_id: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    valid_from: row.get(3)?,
                    valid_to: row.get(4)?,
                })
            },
        )
        .optional()
    }
}

/// Who and where an equal-timestamp reconciliation is audited for.
pub(crate) struct Audited<'a> {
    pub enabled: bool,
    pub scope: &'a str,
    pub session: &'a str,
    pub actor: &'a str,
}

/// Applies `incoming` to the stored row of `id` when it ranks higher than the
/// stored lifecycle. Returns whether the row changed.
///
/// # Errors
/// The SQLite error.
pub(crate) fn reconcile_equal(
    conn: &Connection,
    id: &str,
    incoming: &Lifecycle,
    audited: &Audited<'_>,
) -> rusqlite::Result<bool> {
    let Some(stored) = Lifecycle::stored(conn, id)? else {
        return Ok(false);
    };
    let (winner, loser) = (incoming.key(), stored.key());
    if winner <= loser {
        return Ok(false);
    }
    conn.execute(
        "UPDATE memories SET retired_at=?, superseded_by=?, consolidated_into_id=?, valid_from=?, valid_to=? WHERE id=?",
        params![
            incoming.retired_at,
            (!incoming.superseded_by.is_empty()).then_some(&incoming.superseded_by),
            (!incoming.consolidated_into_id.is_empty()).then_some(&incoming.consolidated_into_id),
            incoming.valid_from,
            incoming.valid_to,
            id
        ],
    )?;
    if audited.enabled {
        let detail = serde_json::json!({
            "rule": "equal updated_at: greater lifecycle key wins",
            "kept": winner.replace('\u{1f}', "|"),
            "replaced": loser.replace('\u{1f}', "|"),
        })
        .to_string();
        audit(
            conn,
            "sync_lifecycle",
            id,
            audited.scope,
            audited.session,
            audited.actor,
            &detail,
        );
    }
    Ok(true)
}
