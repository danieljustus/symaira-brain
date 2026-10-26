//! Applying the changes a relay returned to the local store (#762).
//!
//! Mirrors the shipped `SyncUpsertMemoryIfNewer` (tombstone check, then
//! `UpsertMemoryIfNewer`) and `ApplyRemoteDelete`, statement for statement:
//! the same columns and defaults, the stored timestamps rendered the way the
//! shipped driver binds a `time.Time`, and the same best-effort audit rows.
//!
//! Everything runs in one transaction. Whenever a step meets something the
//! native store does not reproduce (a stored timestamp in another spelling, a
//! failing statement, an embedding that needs the vector pipeline) the whole
//! transaction is rolled back and the caller hands the invocation to the
//! shipped client, so no half-applied state can be observed.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params};

use crate::cli_write::{audit, go_json};
use crate::model::Store;
use crate::sync_state::{parse_cursor, pending_changes, render_cursor, zero_time};

/// One memory of a `/api/sync/changes` reply, as the shipped `db.Memory`
/// decoder would hold it (absent keys keep their zero values).
#[derive(Debug, Clone, PartialEq)]
pub struct PulledMemory {
    pub id: String,
    pub content: String,
    pub scope: String,
    /// `None` is a nil map, which the shipped encoder writes as `null`.
    pub metadata: Option<BTreeMap<String, String>>,
    /// `None` is a nil slice (`null`), `Some(vec![])` an empty one (`[]`).
    /// Non-empty vectors need the embedding pipeline and are not applied.
    pub embedding: Option<Vec<f32>>,
    pub embedding_source: String,
    pub embedding_model: String,
    pub embedding_quantization: String,
    pub content_hash: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: String,
    pub updated_by: String,
    pub created_session: String,
    pub updated_session: String,
    pub consolidation_status: String,
    pub consolidated_into_id: String,
    pub importance: f64,
    pub valid_from: Option<DateTime<Utc>>,
    pub valid_to: Option<DateTime<Utc>>,
    pub superseded_by: String,
    pub kind: String,
    pub review_status: String,
    pub decay_factor: f64,
    pub retired_at: Option<DateTime<Utc>>,
    pub tier: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub access_count: i64,
    pub last_access: Option<DateTime<Utc>>,
    pub prev_access: Option<DateTime<Utc>>,
}

impl PulledMemory {
    /// A memory whose every field has the shipped decoder's zero value.
    #[must_use]
    pub fn zero() -> Self {
        Self {
            id: String::new(),
            content: String::new(),
            scope: String::new(),
            metadata: None,
            embedding: None,
            embedding_source: String::new(),
            embedding_model: String::new(),
            embedding_quantization: String::new(),
            content_hash: String::new(),
            created_at: zero_time(),
            updated_at: zero_time(),
            created_by: String::new(),
            updated_by: String::new(),
            created_session: String::new(),
            updated_session: String::new(),
            consolidation_status: String::new(),
            consolidated_into_id: String::new(),
            importance: 0.0,
            valid_from: None,
            valid_to: None,
            superseded_by: String::new(),
            kind: String::new(),
            review_status: String::new(),
            decay_factor: 0.0,
            retired_at: None,
            tier: String::new(),
            expires_at: None,
            access_count: 0,
            last_access: None,
            prev_access: None,
        }
    }
}

/// A tombstone of a reply (`db.DeletedMemory`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulledDelete {
    pub id: String,
    pub deleted_at: DateTime<Utc>,
}

/// One reply page: memories are applied before tombstones, in order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PulledPage {
    pub memories: Vec<PulledMemory>,
    pub deleted: Vec<PulledDelete>,
}

/// The counters of a pull (`pulled_memories_applied`, `pulled_deletes_applied`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PullApplied {
    pub memories: u32,
    pub deletes: u32,
}

impl Store {
    /// Applies `pages` in order inside one transaction. With `pending_since`
    /// set, the push stream that would follow the pull is probed afterwards:
    /// if it would send anything, nothing is kept.
    ///
    /// `None` means nothing was changed and the shipped client has to do the
    /// work (an unsupported shape, a failing statement, a pending push).
    #[must_use]
    pub fn apply_pulled(
        &self,
        pages: &[PulledPage],
        pending_since: Option<DateTime<Utc>>,
    ) -> Option<PullApplied> {
        let conn = self.lock().ok()?;
        let transaction = conn.unchecked_transaction().ok()?;
        let mut applied = PullApplied::default();
        for page in pages {
            for memory in &page.memories {
                if upsert_if_newer(&transaction, memory)? {
                    applied.memories += 1;
                }
            }
            for delete in &page.deleted {
                if apply_remote_delete(&transaction, delete)? {
                    applied.deletes += 1;
                }
            }
        }
        if let Some(since) = pending_since
            && pending_changes(&transaction, since).ok()?
        {
            return None;
        }
        transaction.commit().ok()?;
        Some(applied)
    }
}

/// `time.Time` as the shipped driver reads it back from a column: its own
/// rendering, or the `strftime` form the oplog trigger writes. Other spellings
/// are not claimed.
fn stored_time(value: &Value) -> Option<DateTime<Utc>> {
    let Value::Text(text) = value else {
        return None;
    };
    if let Some(time) = parse_cursor(text) {
        return Some(time);
    }
    let bytes = text.as_bytes();
    if bytes.len() != 24 || bytes[10] != b'T' || bytes[19] != b'.' || bytes[23] != b'Z' {
        return None;
    }
    if !text.is_ascii() {
        return None;
    }
    let naive = NaiveDateTime::parse_from_str(&text[..23], "%Y-%m-%dT%H:%M:%S%.3f").ok()?;
    Some(Utc.from_utc_datetime(&naive))
}

fn optional_time(time: Option<DateTime<Utc>>) -> Option<String> {
    time.map(render_cursor)
}

fn non_null(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

/// `SyncUpsertMemoryIfNewer`: `Some(applied)`, or `None` to hand off.
fn upsert_if_newer(conn: &Connection, memory: &PulledMemory) -> Option<bool> {
    let latest: Option<(String, Value)> = conn
        .query_row(
            "SELECT op, ts FROM sync_oplog WHERE memory_id = ? ORDER BY event_id DESC LIMIT 1",
            [&memory.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .ok()?;
    if let Some((op, ts)) = latest {
        let ts = stored_time(&ts)?;
        if op == "delete" && memory.updated_at <= ts {
            return Some(false);
        }
    }
    upsert(conn, memory)
}

/// The values `UpsertMemoryIfNewer` derives before touching the table.
struct Derived {
    metadata: String,
    embedding: &'static str,
    content_hash: String,
    status: String,
    review: String,
    decay: f64,
}

fn derive(memory: &PulledMemory) -> Option<Derived> {
    let embedding = match &memory.embedding {
        None => "null",
        Some(vector) if vector.is_empty() => "[]",
        Some(_) => return None,
    };
    let default =
        |value: &str, fallback: &str| if value.is_empty() { fallback } else { value }.to_owned();
    Some(Derived {
        metadata: go_json(&memory.metadata).ok()?,
        embedding,
        content_hash: if memory.content_hash.is_empty() {
            crate::write::content_hash(&memory.content)
        } else {
            memory.content_hash.clone()
        },
        status: default(&memory.consolidation_status, "raw"),
        review: default(&memory.review_status, "approved"),
        decay: if memory.decay_factor <= 0.0 || memory.decay_factor > 1.0 {
            1.0
        } else {
            memory.decay_factor
        },
    })
}

/// `UpsertMemoryIfNewer`.
fn upsert(conn: &Connection, memory: &PulledMemory) -> Option<bool> {
    let derived = derive(memory)?;
    let existing: Option<Value> = conn
        .query_row(
            "SELECT updated_at FROM memories WHERE id = ?",
            [&memory.id],
            |row| row.get(0),
        )
        .optional()
        .ok()?;
    let Some(existing) = existing else {
        insert(conn, memory, &derived)?;
        audit(
            conn,
            "set",
            &memory.id,
            &memory.scope,
            &memory.created_session,
            &memory.created_by,
            "",
        );
        return Some(true);
    };
    if memory.updated_at <= stored_time(&existing)? {
        return Some(false);
    }
    update(conn, memory, &derived)?;
    audit(
        conn,
        "update",
        &memory.id,
        &memory.scope,
        &memory.updated_session,
        &memory.updated_by,
        "",
    );
    Some(true)
}

fn insert(conn: &Connection, memory: &PulledMemory, derived: &Derived) -> Option<()> {
    conn.execute(
        "INSERT INTO memories (id, content, scope, metadata, embedding, embedding_binary, embedding_dim, embedding_source, embedding_model, embedding_quantization, content_hash, lsh_hash, created_at, updated_at, created_by, updated_by, created_session, updated_session, consolidation_status, consolidated_into_id, importance, valid_from, valid_to, superseded_by, tier, expires_at, access_count, last_access, prev_access, review_status, kind, decay_factor, retired_at)\n\t\t\t VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            memory.id,
            memory.content,
            memory.scope,
            derived.metadata,
            derived.embedding,
            Option::<Vec<u8>>::None,
            0_i64,
            memory.embedding_source,
            memory.embedding_model,
            memory.embedding_quantization,
            derived.content_hash,
            0_i64,
            render_cursor(memory.created_at),
            render_cursor(memory.updated_at),
            memory.created_by,
            memory.updated_by,
            memory.created_session,
            memory.updated_session,
            derived.status,
            non_null(&memory.consolidated_into_id),
            memory.importance,
            optional_time(memory.valid_from),
            optional_time(memory.valid_to),
            non_null(&memory.superseded_by),
            memory.tier,
            optional_time(memory.expires_at),
            memory.access_count,
            optional_time(memory.last_access),
            optional_time(memory.prev_access),
            derived.review,
            memory.kind,
            derived.decay,
            optional_time(memory.retired_at),
        ],
    )
    .ok()
    .map(|_| ())
}

/// The update leaves `created_*`, `valid_*` and `superseded_by` alone.
fn update(conn: &Connection, memory: &PulledMemory, derived: &Derived) -> Option<()> {
    conn.execute(
        "UPDATE memories SET content=?, scope=?, metadata=?, embedding=?, embedding_binary=?, embedding_dim=?, embedding_source=?, embedding_model=?, embedding_quantization=?, content_hash=?, lsh_hash=?, updated_at=?, updated_by=?, updated_session=?, consolidation_status=?, consolidated_into_id=?, importance=?, tier=?, expires_at=?, access_count=?, last_access=?, prev_access=?, review_status=?, kind=?, decay_factor=?, retired_at=? WHERE id=?",
        params![
            memory.content,
            memory.scope,
            derived.metadata,
            derived.embedding,
            Option::<Vec<u8>>::None,
            0_i64,
            memory.embedding_source,
            memory.embedding_model,
            memory.embedding_quantization,
            derived.content_hash,
            0_i64,
            render_cursor(memory.updated_at),
            memory.updated_by,
            memory.updated_session,
            derived.status,
            non_null(&memory.consolidated_into_id),
            memory.importance,
            memory.tier,
            optional_time(memory.expires_at),
            memory.access_count,
            optional_time(memory.last_access),
            optional_time(memory.prev_access),
            derived.review,
            memory.kind,
            derived.decay,
            optional_time(memory.retired_at),
            memory.id,
        ],
    )
    .ok()
    .map(|_| ())
}

/// `ApplyRemoteDelete`.
fn apply_remote_delete(conn: &Connection, delete: &PulledDelete) -> Option<bool> {
    let local: Option<Value> = conn
        .query_row(
            "SELECT updated_at FROM memories WHERE id = ?",
            [&delete.id],
            |row| row.get(0),
        )
        .optional()
        .ok()?;
    let Some(local) = local else {
        return Some(false);
    };
    if stored_time(&local)? > delete.deleted_at {
        return Some(false);
    }
    // `DeleteMemory`: identity for the audit row, the delete, then the audit.
    let identity: Option<(Option<String>, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT scope, created_by, created_session FROM memories WHERE id = ?",
            [&delete.id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .ok()?;
    let (scope, created_by, created_session) = identity?;
    conn.execute("DELETE FROM memories WHERE id = ?", [&delete.id])
        .ok()?;
    audit(
        conn,
        "delete",
        &delete.id,
        &scope.unwrap_or_default(),
        &created_session.unwrap_or_default(),
        &created_by.unwrap_or_default(),
        "",
    );
    Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        parse_cursor(text).expect("time")
    }

    fn memory(id: &str, updated: &str) -> PulledMemory {
        let mut memory = PulledMemory::zero();
        memory.id = id.into();
        memory.content = format!("content of {id}");
        memory.scope = "global".into();
        memory.updated_at = at(updated);
        memory.created_at = at(updated);
        memory
    }

    fn page(memories: Vec<PulledMemory>, deleted: Vec<PulledDelete>) -> PulledPage {
        PulledPage { memories, deleted }
    }

    fn column(store: &Store, id: &str, column: &str) -> String {
        store
            .lock()
            .unwrap()
            .query_row(
                &format!("SELECT {column} FROM memories WHERE id = ?"),
                [id],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
    }

    fn count(store: &Store, sql: &str) -> i64 {
        store
            .lock()
            .unwrap()
            .query_row(sql, [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn insert_then_update_follows_the_last_writer_rule() {
        let store = Store::open_in_memory().unwrap();
        let first = memory("a", "2026-10-26 01:00:00 +0000 UTC");
        let applied = store
            .apply_pulled(&[page(vec![first.clone()], vec![])], None)
            .unwrap();
        assert_eq!(applied.memories, 1);
        assert_eq!(
            column(&store, "a", "updated_at"),
            "2026-10-26 01:00:00 +0000 UTC"
        );
        assert_eq!(column(&store, "a", "metadata"), "null");
        assert_eq!(column(&store, "a", "embedding"), "null");

        // Equal and older timestamps are skipped; a newer one overwrites.
        let mut equal = first.clone();
        equal.content = "equal".into();
        let mut older = first.clone();
        older.updated_at = at("2026-10-26 00:59:59 +0000 UTC");
        older.content = "older".into();
        let mut newer = first;
        newer.updated_at = at("2026-10-26 01:00:00.000000001 +0000 UTC");
        newer.content = "newer".into();
        newer.metadata = Some(BTreeMap::from([("k".to_owned(), "<v>".to_owned())]));
        let applied = store
            .apply_pulled(&[page(vec![equal, older, newer], vec![])], None)
            .unwrap();
        assert_eq!(applied.memories, 1);
        assert_eq!(column(&store, "a", "content"), "newer");
        assert_eq!(column(&store, "a", "metadata"), r#"{"k":"\u003cv\u003e"}"#);
        assert_eq!(
            column(&store, "a", "updated_at"),
            "2026-10-26 01:00:00.000000001 +0000 UTC"
        );
        assert_eq!(
            count(&store, "SELECT COUNT(*) FROM audit_log WHERE action='set'"),
            1
        );
        assert_eq!(
            count(
                &store,
                "SELECT COUNT(*) FROM audit_log WHERE action='update'"
            ),
            1
        );
    }

    #[test]
    fn a_newer_tombstone_blocks_resurrection() {
        let store = Store::open_in_memory().unwrap();
        store
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO sync_oplog(op,memory_id,ts) VALUES('delete','gone','2026-10-26 00:00:00 +0000 UTC')",
                [],
            )
            .unwrap();
        let stale = memory("gone", "2026-10-26 00:00:00 +0000 UTC");
        let fresh = memory("gone", "2026-10-26 00:00:01 +0000 UTC");
        let applied = store
            .apply_pulled(&[page(vec![stale], vec![])], None)
            .unwrap();
        assert_eq!(applied.memories, 0);
        assert_eq!(count(&store, "SELECT COUNT(*) FROM memories"), 0);
        let applied = store
            .apply_pulled(&[page(vec![fresh], vec![])], None)
            .unwrap();
        assert_eq!(applied.memories, 1);
    }

    #[test]
    fn remote_deletes_respect_the_local_timestamp() {
        let store = Store::open_in_memory().unwrap();
        let kept = memory("kept", "2026-10-26 02:00:00 +0000 UTC");
        let removed = memory("removed", "2026-10-26 01:00:00 +0000 UTC");
        store
            .apply_pulled(&[page(vec![kept, removed], vec![])], None)
            .unwrap();
        let deletes = vec![
            PulledDelete {
                id: "kept".into(),
                deleted_at: at("2026-10-26 01:59:59 +0000 UTC"),
            },
            PulledDelete {
                id: "removed".into(),
                deleted_at: at("2026-10-26 01:00:00 +0000 UTC"),
            },
            PulledDelete {
                id: "missing".into(),
                deleted_at: at("2026-10-26 03:00:00 +0000 UTC"),
            },
        ];
        let applied = store.apply_pulled(&[page(vec![], deletes)], None).unwrap();
        assert_eq!((applied.memories, applied.deletes), (0, 1));
        assert_eq!(count(&store, "SELECT COUNT(*) FROM memories"), 1);
        assert_eq!(
            count(
                &store,
                "SELECT COUNT(*) FROM audit_log WHERE action='delete'"
            ),
            1
        );
    }

    #[test]
    fn unsupported_state_rolls_everything_back() {
        let store = Store::open_in_memory().unwrap();
        let fine = memory("fine", "2026-10-26 01:00:00 +0000 UTC");
        let mut vector = memory("vector", "2026-10-26 01:00:00 +0000 UTC");
        vector.embedding = Some(vec![0.5]);
        assert!(
            store
                .apply_pulled(&[page(vec![fine.clone(), vector], vec![])], None)
                .is_none()
        );
        assert_eq!(count(&store, "SELECT COUNT(*) FROM memories"), 0);

        // A push that the pull would provoke keeps nothing either.
        assert!(
            store
                .apply_pulled(&[page(vec![fine], vec![])], Some(zero_time()))
                .is_none()
        );
        assert_eq!(count(&store, "SELECT COUNT(*) FROM memories"), 0);

        // A stored time in another spelling is not claimed.
        store
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO memories(id,content,scope,metadata,embedding,created_at,updated_at) VALUES('odd','c','global','{}','[]','x','2026-10-26 03:00:00 +0200 +0200')",
                [],
            )
            .unwrap();
        let odd = memory("odd", "2030-01-01 00:00:00 +0000 UTC");
        assert!(
            store
                .apply_pulled(&[page(vec![odd], vec![])], None)
                .is_none()
        );
    }

    #[test]
    fn stored_times_cover_the_driver_and_the_trigger_spelling() {
        let text = |value: &str| Value::Text(value.to_owned());
        assert!(stored_time(&text("2026-10-26 01:02:03.5 +0000 UTC")).is_some());
        assert_eq!(
            stored_time(&text("2026-10-26T01:02:03.456Z")),
            parse_cursor("2026-10-26 01:02:03.456 +0000 UTC")
        );
        for other in ["2026-10-26T01:02:03Z", "2026-10-26 01:02:03", "", "x"] {
            assert!(stored_time(&text(other)).is_none(), "{other}");
        }
        assert!(stored_time(&Value::Integer(1)).is_none());
    }
}
