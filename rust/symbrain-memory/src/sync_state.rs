//! Per-remote sync cursor and the "anything to push?" probe used by the
//! native `memory sync` client (#762).
//!
//! The shipped driver stores a `time.Time` as
//! `2006-01-02 15:04:05.999999999 +0000 UTC` (nanosecond precision, trailing
//! zeros trimmed) and only reads such text back as a time; every other
//! spelling fails the shipped cursor read. The native store therefore reads
//! exactly this rendering and reports anything else as unsupported so the
//! caller keeps the shipped implementation.

use chrono::{DateTime, NaiveDateTime, TimeZone, Timelike, Utc};
use rusqlite::OptionalExtension;
use rusqlite::types::Value;

use crate::model::{Store, StoreError};

/// What `sync_state` holds for one remote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredCursor {
    /// No row: the shipped client starts from the zero time.
    Absent,
    /// A row in the shipped rendering.
    At(DateTime<Utc>),
    /// A row whose spelling the shipped driver would not read as a time.
    Unsupported,
}

/// The Go zero `time.Time` (`0001-01-01T00:00:00Z`).
#[must_use]
pub fn zero_time() -> DateTime<Utc> {
    // -62135596800 is 0001-01-01T00:00:00Z; the fallback is unreachable.
    DateTime::from_timestamp(-62_135_596_800, 0).unwrap_or(DateTime::UNIX_EPOCH)
}

/// Renders an instant the way the shipped driver stores it.
#[must_use]
pub fn render_cursor(time: DateTime<Utc>) -> String {
    let base = time.format("%Y-%m-%d %H:%M:%S").to_string();
    let nanos = time.nanosecond();
    if nanos == 0 {
        return format!("{base} +0000 UTC");
    }
    let fraction = format!("{nanos:09}");
    format!("{base}.{} +0000 UTC", fraction.trim_end_matches('0'))
}

/// Parses exactly the shipped rendering (UTC only, one to nine fraction digits).
#[must_use]
pub fn parse_cursor(raw: &str) -> Option<DateTime<Utc>> {
    let body = raw.strip_suffix(" +0000 UTC")?;
    let (stamp, fraction) = match body.split_once('.') {
        Some((stamp, fraction)) => (stamp, Some(fraction)),
        None => (body, None),
    };
    if stamp.len() != 19 || !stamp.is_ascii() {
        return None;
    }
    let bytes = stamp.as_bytes();
    let digit_positions = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18];
    let separators = [(4, b'-'), (7, b'-'), (10, b' '), (13, b':'), (16, b':')];
    if !digit_positions.iter().all(|&at| bytes[at].is_ascii_digit())
        || !separators.iter().all(|&(at, byte)| bytes[at] == byte)
    {
        return None;
    }
    let naive = NaiveDateTime::parse_from_str(stamp, "%Y-%m-%d %H:%M:%S").ok()?;
    let nanos = match fraction {
        None => 0,
        Some(digits) => {
            if digits.is_empty()
                || digits.len() > 9
                || !digits.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            format!("{digits:0<9}").parse::<u32>().ok()?
        }
    };
    Some(Utc.from_utc_datetime(&naive.with_nanosecond(nanos)?))
}

impl Store {
    /// Reads the cursor stored for `remote` (`GetSyncCursor`).
    ///
    /// # Errors
    /// Returns a `StoreError` when the table cannot be read.
    pub fn sync_cursor(&self, remote: &str) -> Result<StoredCursor, StoreError> {
        let conn = self.lock()?;
        let stored: Option<Value> = conn
            .query_row(
                "SELECT last_sync FROM sync_state WHERE remote = ?",
                [remote],
                |row| row.get(0),
            )
            .optional()?;
        Ok(match stored {
            None => StoredCursor::Absent,
            Some(Value::Text(text)) => {
                parse_cursor(&text).map_or(StoredCursor::Unsupported, StoredCursor::At)
            }
            Some(_) => StoredCursor::Unsupported,
        })
    }

    /// Upserts the cursor for `remote` (`SetSyncCursor`).
    ///
    /// # Errors
    /// Returns a `StoreError` when the write fails.
    pub fn set_sync_cursor(&self, remote: &str, at: DateTime<Utc>) -> Result<(), StoreError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO sync_state (remote, last_sync) VALUES (?, ?)\n\t\t ON CONFLICT(remote) DO UPDATE SET last_sync = excluded.last_sync",
            rusqlite::params![remote, render_cursor(at)],
        )?;
        Ok(())
    }

    /// Whether the shipped push stream would send anything after `since`:
    /// memories (not marked `sync_exclude`) updated later, or a tombstone
    /// recorded later. Uses the shipped predicates with the shipped bound
    /// rendering, so the string comparisons agree with the shipped queries.
    ///
    /// # Errors
    /// Returns a `StoreError` when a probe query fails (for example on
    /// malformed metadata), in which case the shipped client would fail too.
    pub fn has_pending_sync_changes(&self, since: DateTime<Utc>) -> Result<bool, StoreError> {
        let conn = self.lock()?;
        pending_changes(&conn, since)
    }
}

/// The probe behind [`Store::has_pending_sync_changes`], usable inside a
/// transaction.
pub(crate) fn pending_changes(
    conn: &rusqlite::Connection,
    since: DateTime<Utc>,
) -> Result<bool, StoreError> {
    let bound = render_cursor(since);
    let memories: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM memories WHERE (updated_at > ?) AND COALESCE(json_extract(metadata, '$.sync_exclude'), '') != 'true' LIMIT 1",
            [&bound],
            |row| row.get(0),
        )
        .optional()?;
    if memories.is_some() {
        return Ok(true);
    }
    let deleted: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM sync_oplog o WHERE o.op = 'delete' AND o.ts > ? AND o.event_id = (SELECT MAX(event_id) FROM sync_oplog WHERE memory_id = o.memory_id) LIMIT 1",
            [&bound],
            |row| row.get(0),
        )
        .optional()?;
    Ok(deleted.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_time_is_the_go_zero_value() {
        assert_eq!(render_cursor(zero_time()), "0001-01-01 00:00:00 +0000 UTC");
    }

    #[test]
    fn renders_and_parses_the_shipped_form() {
        for raw in [
            "0001-01-01 00:00:00 +0000 UTC",
            "2026-10-26 01:02:03 +0000 UTC",
            "2026-10-26 01:02:03.123456789 +0000 UTC",
            "2026-10-26 01:02:03.5 +0000 UTC",
        ] {
            let parsed = parse_cursor(raw).unwrap();
            assert_eq!(render_cursor(parsed), raw);
        }
        assert_eq!(
            parse_cursor("0001-01-01 00:00:00 +0000 UTC"),
            Some(zero_time())
        );
    }

    #[test]
    fn rejects_every_other_spelling() {
        for raw in [
            "2026-10-26 03:02:03.456 +0200 +0200",
            "2026-10-26T01:02:03Z",
            "2026-10-26 01:02:03",
            "2026-10-26 01:02:03. +0000 UTC",
            "2026-10-26 01:02:03.1234567890 +0000 UTC",
            "2026-13-26 01:02:03 +0000 UTC",
            " 2026-10-26 01:02:03 +0000 UTC",
            "2026-10-26 01:02:03 +0000 UTC ",
            "",
        ] {
            assert!(parse_cursor(raw).is_none(), "accepted: {raw:?}");
        }
    }

    #[test]
    fn cursor_roundtrip_and_pending_probe() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.sync_cursor("r").unwrap(), StoredCursor::Absent);
        let at = parse_cursor("2026-10-26 01:02:03.25 +0000 UTC").unwrap();
        store.set_sync_cursor("r", at).unwrap();
        assert_eq!(store.sync_cursor("r").unwrap(), StoredCursor::At(at));
        store.set_sync_cursor("r", zero_time()).unwrap();
        assert_eq!(
            store.sync_cursor("r").unwrap(),
            StoredCursor::At(zero_time())
        );
        assert!(!store.has_pending_sync_changes(zero_time()).unwrap());
        store
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO sync_state (remote, last_sync) VALUES ('odd', '2026-10-26 03:02:03 +0200 +0200')",
                [],
            )
            .unwrap();
        assert_eq!(store.sync_cursor("odd").unwrap(), StoredCursor::Unsupported);
    }
}
