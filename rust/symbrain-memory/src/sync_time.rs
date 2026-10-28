//! `symbrain_sync_time`: the SQLite scalar function the shipped Go store
//! registers (`internal/memory/db/sync_time.go`, #980) so sync cursor
//! predicates compare driver-written, trigger-written and legacy timestamps
//! chronologically instead of as text.
//!
//! [`normalize_sync_time`] reproduces `normalizeSyncTime` including the
//! `time.Parse` rules it relies on (fixed-width two-digit fields, a one- or
//! two-digit hour, any number of fraction digits with only nine kept, range
//! checks, calendar-valid days). Layouts that are only reachable through the
//! Go zone-abbreviation fallback are covered by the numeric-offset branch.

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::functions::FunctionFlags;
use rusqlite::types::ValueRef;

/// The registered SQL function name.
pub(crate) const FUNCTION: &str = "symbrain_sync_time";

/// Registers [`FUNCTION`] on `conn` (Go registers it process-wide before the
/// connection opens; the native store registers it right after opening).
pub(crate) fn register(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.create_scalar_function(
        FUNCTION,
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let ValueRef::Text(bytes) = ctx.get_raw(0) else {
                return Err(user_error("sync timestamp is not text"));
            };
            let raw = std::str::from_utf8(bytes).map_err(|error| user_error(&error.to_string()))?;
            normalize_sync_time(raw).map_err(|message| user_error(&message))
        },
    )
}

fn user_error(message: &str) -> rusqlite::Error {
    rusqlite::Error::UserFunctionError(message.to_owned().into())
}

/// Fixed-width UTC text with nanoseconds, or Go's `invalid sync timestamp`.
///
/// # Errors
/// Returns the Go error text when no supported layout parses `raw`.
pub(crate) fn normalize_sync_time(raw: &str) -> Result<String, String> {
    // Go: `strings.Cut(raw, " m=")` drops a monotonic-clock suffix.
    let text = raw.split_once(" m=").map_or(raw, |(head, _)| head);
    // Go: a four-field `time.String()` carries a numeric offset before the
    // zone abbreviation, which is authoritative.
    let fields: Vec<&str> = text.split_whitespace().collect();
    if fields.len() == 4
        && let Some(done) = parse(&fields[..3].join(" "), Layout::SpaceOffset)
    {
        return render(&done);
    }
    let trimmed = text.trim().strip_suffix('Z').unwrap_or(text.trim());
    for layout in [
        Layout::Rfc3339Nano,
        Layout::SpaceColonOffset,
        Layout::SpaceNaive,
        Layout::TNaive,
        Layout::SpaceMinute,
        Layout::TMinute,
        Layout::Date,
    ] {
        if let Some(done) = parse(trimmed, layout) {
            return render(&done);
        }
    }
    Err(format!("invalid sync timestamp {raw:?}"))
}

#[derive(Clone, Copy)]
enum Layout {
    /// `2006-01-02 15:04:05.999999999 -0700`
    SpaceOffset,
    /// `2006-01-02T15:04:05.999999999Z07:00`
    Rfc3339Nano,
    /// `2006-01-02 15:04:05.999999999-07:00`
    SpaceColonOffset,
    /// `2006-01-02 15:04:05.999999999`
    SpaceNaive,
    /// `2006-01-02T15:04:05.999999999`
    TNaive,
    /// `2006-01-02 15:04`
    SpaceMinute,
    /// `2006-01-02T15:04`
    TMinute,
    /// `2006-01-02`
    Date,
}

/// A parsed instant as UTC nanoseconds-since-epoch parts.
struct Parsed {
    local: chrono::NaiveDateTime,
    offset_seconds: i64,
}

struct Cursor<'a> {
    rest: &'a [u8],
}

impl Cursor<'_> {
    fn digit(&self, at: usize) -> bool {
        self.rest.get(at).is_some_and(u8::is_ascii_digit)
    }

    /// Go `getnum`: one or two digits, exactly two when `fixed`.
    fn number(&mut self, fixed: bool) -> Option<i64> {
        if !self.digit(0) {
            return None;
        }
        if !self.digit(1) {
            if fixed {
                return None;
            }
            let value = i64::from(self.rest[0] - b'0');
            self.rest = &self.rest[1..];
            return Some(value);
        }
        let value = i64::from(self.rest[0] - b'0') * 10 + i64::from(self.rest[1] - b'0');
        self.rest = &self.rest[2..];
        Some(value)
    }

    fn year(&mut self) -> Option<i64> {
        if self.rest.len() < 4 || !self.rest[..4].iter().all(u8::is_ascii_digit) {
            return None;
        }
        let value = self.rest[..4]
            .iter()
            .fold(0_i64, |sum, byte| sum * 10 + i64::from(byte - b'0'));
        self.rest = &self.rest[4..];
        Some(value)
    }

    /// A literal layout byte. Go's `skip` lets one layout space consume a
    /// whole run of input spaces (not tabs).
    fn literal(&mut self, byte: u8) -> Option<()> {
        if self.rest.first() != Some(&byte) {
            return None;
        }
        if byte == b' ' {
            while self.rest.first() == Some(&b' ') {
                self.rest = &self.rest[1..];
            }
        } else {
            self.rest = &self.rest[1..];
        }
        Some(())
    }

    /// Optional `.fffffffff` (Go also accepts a comma); every digit is
    /// consumed and only the first nine are kept.
    fn fraction(&mut self) -> u32 {
        let separated = matches!(self.rest.first(), Some(b'.' | b','));
        if self.rest.len() < 2 || !separated || !self.digit(1) {
            return 0;
        }
        let mut digits = 0;
        while self.digit(1 + digits) {
            digits += 1;
        }
        let mut nanos = 0_u32;
        for at in 0..9 {
            nanos *= 10;
            if at < digits {
                nanos += u32::from(self.rest[1 + at] - b'0');
            }
        }
        self.rest = &self.rest[1 + digits..];
        nanos
    }

    /// `-0700` (`colon == false`) or `-07:00` / `Z07:00`.
    fn zone(&mut self, colon: bool, allow_z: bool) -> Option<i64> {
        if allow_z && self.rest.first() == Some(&b'Z') {
            self.rest = &self.rest[1..];
            return Some(0);
        }
        let width = if colon { 6 } else { 5 };
        if self.rest.len() < width {
            return None;
        }
        let sign = match self.rest[0] {
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        let (hour, minute) = if colon {
            if self.rest[3] != b':' {
                return None;
            }
            (&self.rest[1..3], &self.rest[4..6])
        } else {
            (&self.rest[1..3], &self.rest[3..5])
        };
        let two = |pair: &[u8]| {
            if pair.iter().all(u8::is_ascii_digit) {
                Some(i64::from(pair[0] - b'0') * 10 + i64::from(pair[1] - b'0'))
            } else {
                None
            }
        };
        let (hours, minutes) = (two(hour)?, two(minute)?);
        if hours > 24 || minutes > 60 {
            return None;
        }
        self.rest = &self.rest[width..];
        Some(sign * (hours * 60 + minutes) * 60)
    }
}

fn parse(input: &str, layout: Layout) -> Option<Parsed> {
    let mut cursor = Cursor {
        rest: input.as_bytes(),
    };
    let year = cursor.year()?;
    cursor.literal(b'-')?;
    let month = cursor.number(true)?;
    cursor.literal(b'-')?;
    let day = cursor.number(true)?;
    let (mut hour, mut minute, mut second, mut nanos, mut offset) = (0, 0, 0, 0, 0);
    if !matches!(layout, Layout::Date) {
        let separator = match layout {
            Layout::Rfc3339Nano | Layout::TNaive | Layout::TMinute => b'T',
            _ => b' ',
        };
        cursor.literal(separator)?;
        hour = cursor.number(false)?;
        cursor.literal(b':')?;
        minute = cursor.number(true)?;
        if !matches!(layout, Layout::SpaceMinute | Layout::TMinute) {
            cursor.literal(b':')?;
            second = cursor.number(true)?;
            nanos = cursor.fraction();
        }
        match layout {
            Layout::SpaceOffset => {
                cursor.literal(b' ')?;
                offset = cursor.zone(false, false)?;
            }
            Layout::Rfc3339Nano => offset = cursor.zone(true, true)?,
            Layout::SpaceColonOffset => offset = cursor.zone(true, false)?,
            _ => {}
        }
    }
    if !cursor.rest.is_empty() || !(1..=12).contains(&month) {
        return None;
    }
    if !(0..24).contains(&hour) || !(0..60).contains(&minute) || !(0..60).contains(&second) {
        return None;
    }
    let date = NaiveDate::from_ymd_opt(
        i32::try_from(year).ok()?,
        u32::try_from(month).ok()?,
        u32::try_from(day).ok()?,
    )?;
    let local = date.and_hms_nano_opt(
        u32::try_from(hour).ok()?,
        u32::try_from(minute).ok()?,
        u32::try_from(second).ok()?,
        nanos,
    )?;
    Some(Parsed {
        local,
        offset_seconds: offset,
    })
}

fn render(parsed: &Parsed) -> Result<String, String> {
    let utc = parsed
        .local
        .checked_sub_signed(chrono::Duration::seconds(parsed.offset_seconds))
        .ok_or_else(|| "sync timestamp out of range".to_owned())?;
    let instant = DateTime::<Utc>::from_naive_utc_and_offset(utc, Utc);
    // Go's `2006` prints at least four digits and a sign for negative years.
    let year = chrono::Datelike::year(&instant);
    let year = if year < 0 {
        format!("-{:04}", -i64::from(year))
    } else {
        format!("{year:04}")
    };
    Ok(format!("{year}{}", instant.format("-%m-%dT%H:%M:%S%.9fZ")))
}

#[cfg(test)]
mod tests {
    use super::normalize_sync_time;
    use crate::Store;
    use serde_json::Value;

    /// Recorded from the Go function by
    /// `scripts/memory-cli-oracle/sync_time_fixture.py` (`--check` in CI).
    fn fixture() -> Vec<(String, Option<String>)> {
        let document: Value =
            serde_json::from_str(include_str!("../tests/fixtures/sync_time_go.json"))
                .expect("recorded Go sync time fixture is JSON");
        document["vectors"]
            .as_array()
            .expect("vectors")
            .iter()
            .map(|vector| {
                let input = vector["input"].as_str().expect("input").to_owned();
                let output = (!vector["error"].as_bool().expect("error flag"))
                    .then(|| vector["output"].as_str().expect("output").to_owned());
                (input, output)
            })
            .collect()
    }

    #[test]
    fn every_recorded_go_vector_matches() {
        let vectors = fixture();
        assert!(vectors.len() >= 100, "fixture shrank to {}", vectors.len());
        assert!(vectors.iter().any(|(_, output)| output.is_some()));
        assert!(vectors.iter().any(|(_, output)| output.is_none()));
        for (input, expected) in &vectors {
            assert_eq!(
                normalize_sync_time(input).ok(),
                *expected,
                "input {input:?}"
            );
        }
    }

    #[test]
    fn registered_sql_function_matches_and_rejects_non_text() {
        let store = Store::open_in_memory().expect("store");
        let conn = store.lock().expect("lock");
        for (input, expected) in fixture() {
            let got: rusqlite::Result<String> =
                conn.query_row("SELECT symbrain_sync_time(?)", [&input], |row| row.get(0));
            assert_eq!(got.ok(), expected, "SQL input {input:?}");
        }
        for bad in ["NULL", "1", "1.5", "X'00'"] {
            let sql = format!("SELECT symbrain_sync_time({bad})");
            let got: rusqlite::Result<String> = conn.query_row(&sql, [], |row| row.get(0));
            assert!(got.is_err(), "{bad} must fail like Go's non-text error");
        }
    }

    #[test]
    fn pending_probe_compares_instants_not_text() {
        use crate::sync_state::{parse_cursor, pending_changes};
        let store = Store::open_in_memory().expect("store");
        let conn = store.lock().expect("lock");
        let since = parse_cursor("2026-01-01 00:00:00.100000001 +0000 UTC").expect("cursor");
        conn.execute(
            "INSERT INTO sync_oplog(op, memory_id, ts) VALUES ('delete', 'a', ?)",
            ["2026-01-01T01:00:00.100000001+01:00"],
        )
        .expect("equivalent instant in another layout");
        assert!(!pending_changes(&conn, since).expect("probe"));
        conn.execute(
            "INSERT INTO sync_oplog(op, memory_id, ts) VALUES ('delete', 'b', ?)",
            ["2026-01-01 00:00:00.100000002"],
        )
        .expect("one nanosecond later");
        assert!(pending_changes(&conn, since).expect("probe"));
    }
}
