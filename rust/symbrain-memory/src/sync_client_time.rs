//! Go's `time.Time` as the sync client observes it: an instant together with
//! the zone it was spelled in.
//!
//! The shipped client keeps the zone of every timestamp it decodes (`"Z"` is
//! UTC, `"+02:00"` a fixed zone) and of every timestamp it reads back from the
//! database, writes the `Time.String()` rendering of that zone into the
//! store, and prints the zone again in its JSON report. Comparisons only look
//! at the instant, but `maxTime` returns its *second* argument on a tie, so the
//! zone decides which spelling survives. [`GoTime`] keeps that information.
//!
//! The process-local zone (`time.Local`) is treated as UTC: an offset spelling
//! is never mapped to a local abbreviation such as `CET`.

use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Timelike, Utc};

/// An instant plus the zone Go would print it in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoTime {
    instant: DateTime<Utc>,
    /// Seconds east of UTC.
    offset: i32,
    /// The abbreviation `Time.String()` prints; empty for an unnamed fixed zone.
    zone: String,
}

fn zero_instant() -> DateTime<Utc> {
    DateTime::from_timestamp(-62_135_596_800, 0).unwrap_or(DateTime::UNIX_EPOCH)
}

impl GoTime {
    /// `time.Time{}`.
    #[must_use]
    pub fn zero() -> Self {
        Self::utc(zero_instant())
    }

    /// An instant in UTC.
    #[must_use]
    pub fn utc(instant: DateTime<Utc>) -> Self {
        Self {
            instant,
            offset: 0,
            zone: "UTC".to_owned(),
        }
    }

    fn fixed(instant: DateTime<Utc>, offset: i32, zone: &str) -> Self {
        let mut time = Self {
            instant,
            offset,
            zone: String::new(),
        };
        // A zone named after its own offset prints like an unnamed one.
        if zone != time.offset_text(false) {
            zone.clone_into(&mut time.zone);
        }
        time
    }

    /// The instant.
    #[must_use]
    pub fn instant(&self) -> DateTime<Utc> {
        self.instant
    }

    /// `Time.IsZero`.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.instant == zero_instant()
    }

    /// `Time.After`.
    #[must_use]
    pub fn after(&self, other: &Self) -> bool {
        self.instant > other.instant
    }

    /// The sync client's `maxTime(a, b)`: `a` only when strictly later.
    #[must_use]
    pub fn max_of(first: Self, second: Self) -> Self {
        if first.after(&second) { first } else { second }
    }

    fn local(&self) -> chrono::NaiveDateTime {
        self.instant.naive_utc() + chrono::Duration::seconds(i64::from(self.offset))
    }

    fn offset_text(&self, colon: bool) -> String {
        let minutes = self.offset.abs() / 60;
        format!(
            "{}{:02}{}{:02}",
            if self.offset < 0 { '-' } else { '+' },
            minutes / 60,
            if colon { ":" } else { "" },
            minutes % 60
        )
    }

    fn fraction(nanos: u32, trim: bool) -> String {
        if nanos == 0 && trim {
            return String::new();
        }
        let digits = format!("{nanos:09}");
        format!(".{}", digits.trim_end_matches('0'))
    }

    /// `Time.String()` without a monotonic reading: the form the shipped
    /// driver stores in a text column.
    #[must_use]
    pub fn to_go_string(&self) -> String {
        let local = self.local();
        let zone = if self.zone.is_empty() {
            self.offset_text(false)
        } else {
            self.zone.clone()
        };
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}{} {} {}",
            local.year(),
            local.month(),
            local.day(),
            local.hour(),
            local.minute(),
            local.second(),
            Self::fraction(local.nanosecond(), true),
            self.offset_text(false),
            zone
        )
    }

    /// `Time.MarshalJSON` (RFC 3339 with nanoseconds, the zone's own offset).
    #[must_use]
    pub fn to_rfc3339_nano(&self) -> String {
        let local = self.local();
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{}",
            local.year(),
            local.month(),
            local.day(),
            local.hour(),
            local.minute(),
            local.second(),
            Self::fraction(local.nanosecond(), true),
            if self.offset == 0 {
                "Z".to_owned()
            } else {
                self.offset_text(true)
            }
        )
    }

    /// `t.UTC().Format(time.RFC3339)`.
    #[must_use]
    pub fn to_rfc3339_utc(&self) -> String {
        let utc = self.instant.naive_utc();
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            utc.year(),
            utc.month(),
            utc.day(),
            utc.hour(),
            utc.minute(),
            utc.second()
        )
    }

    /// `Time.Format(time.RFC3339Nano)` after `UTC()`, as the relay query uses.
    #[must_use]
    pub fn to_rfc3339_nano_utc(&self) -> String {
        Self::utc(self.instant).to_rfc3339_nano()
    }
}

fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in(month: u32, year: i32) -> u32 {
    match month {
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// `strconv.Quote` for the characters that occur in time diagnostics (Go's
/// `time.quote` hex-escapes every non-ASCII and control byte).
fn time_quote(text: &str) -> String {
    let mut out = String::from("\"");
    for &byte in text.as_bytes() {
        match byte {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7e => out.push(char::from(byte)),
            0x0a => out.push_str("\\n"),
            0x0d => out.push_str("\\r"),
            0x09 => out.push_str("\\t"),
            other => {
                out.push_str("\\x");
                out.push(char::from(b"0123456789abcdef"[usize::from(other >> 4)]));
                out.push(char::from(b"0123456789abcdef"[usize::from(other & 15)]));
            }
        }
    }
    out.push('"');
    out
}

const RFC3339: &str = "2006-01-02T15:04:05Z07:00";

fn cannot_parse(value: &str, remaining: &str, element: &str) -> String {
    format!(
        "parsing time {} as {}: cannot parse {} as {}",
        time_quote(value),
        time_quote(RFC3339),
        time_quote(remaining),
        time_quote(element)
    )
}

fn out_of_range(value: &str, what: &str) -> String {
    format!("parsing time {}: {what} out of range", time_quote(value))
}

/// `getnum`: one or two digits (exactly two when `fixed`).
fn getnum(text: &str, fixed: bool) -> Option<(u32, &str)> {
    let bytes = text.as_bytes();
    if !bytes.first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    if !bytes.get(1).is_some_and(u8::is_ascii_digit) {
        if fixed {
            return None;
        }
        return Some((u32::from(bytes[0] - b'0'), &text[1..]));
    }
    Some((
        u32::from(bytes[0] - b'0') * 10 + u32::from(bytes[1] - b'0'),
        &text[2..],
    ))
}

/// `time.Parse(time.RFC3339, value)` (which `Time.UnmarshalJSON` falls back to
/// and which accepts a superset of its fast path), with Go's wording for every
/// failure. `Ok` carries the instant in the zone the text spelled.
///
/// # Errors
/// The `*time.ParseError` text.
#[allow(clippy::too_many_lines)] // One transcription of Go's `parseRFC3339`.
pub fn parse_rfc3339(value: &str) -> Result<GoTime, String> {
    let mut rest = value;
    // stdLongYear
    let hold = rest;
    let digits = rest.as_bytes();
    if digits.len() < 4 || !digits[0].is_ascii_digit() {
        return Err(cannot_parse(value, hold, "2006"));
    }
    let Some(year) = rest.get(..4).and_then(|part| {
        part.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| part.parse::<i32>().ok())
            .flatten()
    }) else {
        return Err(cannot_parse(value, hold, "2006"));
    };
    rest = &rest[4..];

    let mut field = |prefix: &str, element: &str, fixed: bool| -> Result<(u32, bool), String> {
        let Some(after) = rest.strip_prefix(prefix) else {
            return Err(cannot_parse(value, rest, prefix));
        };
        let hold = after;
        let Some((number, tail)) = getnum(after, fixed) else {
            return Err(cannot_parse(value, hold, element));
        };
        rest = tail;
        Ok((number, false))
    };
    let (month, _) = field("-", "01", true)?;
    if month == 0 || month > 12 {
        return Err(out_of_range(value, "month"));
    }
    let (day, _) = field("-", "02", true)?;
    let (hour, _) = field("T", "15", false)?;
    if hour >= 24 {
        return Err(out_of_range(value, "hour"));
    }
    let (minute, _) = field(":", "04", true)?;
    if minute >= 60 {
        return Err(out_of_range(value, "minute"));
    }
    let (second, _) = field(":", "05", true)?;
    if second >= 60 {
        return Err(out_of_range(value, "second"));
    }
    // A fractional second the layout does not name.
    let mut nanos = 0_u32;
    let bytes = rest.as_bytes();
    if bytes.len() >= 2 && matches!(bytes[0], b'.' | b',') && bytes[1].is_ascii_digit() {
        let mut end = 2;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        let kept = &rest[1..end.min(10)];
        let mut scaled: u32 = kept.parse().unwrap_or(0);
        for _ in kept.len()..9 {
            scaled *= 10;
        }
        nanos = scaled;
        rest = &rest[end..];
    }
    // stdISO8601ColonTZ
    let mut offset = 0_i32;
    let mut utc = false;
    let hold = rest;
    if let Some(after) = rest.strip_prefix('Z') {
        rest = after;
        utc = true;
    } else {
        let bytes = rest.as_bytes();
        if bytes.len() < 6 || bytes[3] != b':' {
            return Err(cannot_parse(value, hold, "Z07:00"));
        }
        let number = |part: &str| -> Option<i32> {
            (part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| part.parse().ok())
                .flatten()
        };
        let (Some(hours), Some(minutes)) = (number(&rest[1..3]), number(&rest[4..6])) else {
            return Err(cannot_parse(value, hold, "Z07:00"));
        };
        let sign = bytes[0];
        rest = &rest[6..];
        if hours > 24 {
            return Err(out_of_range(value, "time zone offset hour"));
        }
        if minutes > 60 {
            return Err(out_of_range(value, "time zone offset minute"));
        }
        offset = (hours * 60 + minutes) * 60;
        match sign {
            b'+' => {}
            b'-' => offset = -offset,
            _ => return Err(cannot_parse(value, hold, "Z07:00")),
        }
    }
    if !rest.is_empty() {
        return Err(format!(
            "parsing time {}: extra text: {}",
            time_quote(value),
            time_quote(rest)
        ));
    }
    if day == 0 || day > days_in(month, year) {
        return Err(format!(
            "parsing time {}: day out of range",
            time_quote(value)
        ));
    }
    let date = NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|date| date.and_hms_nano_opt(hour, minute, second, nanos))
        .ok_or_else(|| format!("parsing time {}: day out of range", time_quote(value)))?;
    let instant = Utc.from_utc_datetime(&date) - chrono::Duration::seconds(i64::from(offset));
    Ok(if utc || offset == 0 {
        // Offset zero is the local zone when the process runs in UTC.
        GoTime::utc(instant)
    } else {
        GoTime::fixed(instant, offset, "")
    })
}

/// `Time.UnmarshalJSON` for the raw JSON token `data`.
///
/// # Errors
/// The text Go reports.
pub fn unmarshal_json(data: &str) -> Result<Option<GoTime>, String> {
    if data == "null" {
        return Ok(None);
    }
    let bytes = data.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || bytes[bytes.len() - 1] != b'"' {
        return Err("Time.UnmarshalJSON: input is not a JSON string".to_owned());
    }
    parse_rfc3339(&data[1..data.len() - 1]).map(Some)
}

/// How the shipped driver reads a text column as a time: its own
/// `Time.String()` rendering, then the SQLite formats 1 through 7.
#[must_use]
pub fn parse_stored(text: &str) -> Option<GoTime> {
    let string_form = text
        .split_once("m=")
        .map_or(text, |(head, _)| head)
        .trim_matches(|c: char| c.is_ascii_whitespace() || c == '\u{b}' || c == '\u{c}');
    if let Some(time) = parse_string_form(string_form) {
        return Some(time);
    }
    let (body, had_z) = text
        .strip_suffix('Z')
        .map_or((text, false), |body| (body, true));
    let _ = had_z;
    parse_sqlite_formats(body)
}

fn digits_n(text: &str, count: usize) -> Option<(u32, &str)> {
    let part = text.get(..count)?;
    if !part.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((part.parse().ok()?, &text[count..]))
}

struct Parts<'a> {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    nanos: u32,
    rest: &'a str,
}

/// `2006-01-02`, optionally followed by `T` or space and `15:04[:05[.fff]]`.
fn parse_date_time(text: &str, with_time: bool, with_seconds: bool) -> Option<Parts<'_>> {
    let (year, rest) = digits_n(text, 4)?;
    let rest = rest.strip_prefix('-')?;
    let (month, rest) = digits_n(rest, 2)?;
    let rest = rest.strip_prefix('-')?;
    let (day, rest) = digits_n(rest, 2)?;
    let (mut hour, mut minute, mut second, mut nanos) = (0, 0, 0, 0);
    let mut rest = rest;
    if with_time {
        rest = rest.strip_prefix(' ').or_else(|| rest.strip_prefix('T'))?;
        let (h, tail) = getnum(rest, false)?;
        let tail = tail.strip_prefix(':')?;
        let (m, tail) = digits_n(tail, 2)?;
        hour = h;
        minute = m;
        rest = tail;
        if with_seconds {
            let tail = rest.strip_prefix(':')?;
            let (s, tail) = digits_n(tail, 2)?;
            second = s;
            rest = tail;
            if let Some(after) = rest.strip_prefix('.') {
                let end = after
                    .bytes()
                    .position(|byte| !byte.is_ascii_digit())
                    .unwrap_or(after.len());
                if end == 0 {
                    return None;
                }
                let kept = &after[..end.min(9)];
                let mut scaled: u32 = kept.parse().ok()?;
                for _ in kept.len()..9 {
                    scaled *= 10;
                }
                nanos = scaled;
                rest = &after[end..];
            }
        }
    }
    let year = i32::try_from(year).ok()?;
    if !(1..=12).contains(&month)
        || day == 0
        || day > days_in(month, year)
        || hour >= 24
        || minute >= 60
        || second >= 60
    {
        return None;
    }
    Some(Parts {
        year,
        month,
        day,
        hour,
        minute,
        second,
        nanos,
        rest,
    })
}

fn assemble(parts: &Parts<'_>, offset: i32, zone: &str) -> Option<GoTime> {
    let naive = NaiveDate::from_ymd_opt(parts.year, parts.month, parts.day)?.and_hms_nano_opt(
        parts.hour,
        parts.minute,
        parts.second,
        parts.nanos,
    )?;
    let instant = Utc.from_utc_datetime(&naive) - chrono::Duration::seconds(i64::from(offset));
    Some(if offset == 0 && (zone == "UTC" || zone.is_empty()) {
        GoTime::utc(instant)
    } else {
        GoTime::fixed(instant, offset, zone)
    })
}

/// `2006-01-02 15:04:05.999999999 -0700 MST`.
fn parse_string_form(text: &str) -> Option<GoTime> {
    let parts = parse_date_time(text, true, true)?;
    if text.as_bytes().get(10).is_none_or(|byte| *byte != b' ') {
        return None;
    }
    let rest = parts.rest.strip_prefix(' ')?;
    let bytes = rest.as_bytes();
    if bytes.len() < 5 || !matches!(bytes[0], b'+' | b'-') {
        return None;
    }
    let hours: i32 = rest.get(1..3)?.parse().ok()?;
    let minutes: i32 = rest.get(3..5)?.parse().ok()?;
    if !rest.get(1..5)?.bytes().all(|byte| byte.is_ascii_digit()) || hours > 24 || minutes > 60 {
        return None;
    }
    let mut offset = (hours * 60 + minutes) * 60;
    if bytes[0] == b'-' {
        offset = -offset;
    }
    let zone = rest[5..].strip_prefix(' ')?;
    if zone.is_empty() || zone.contains(char::is_whitespace) {
        return None;
    }
    assemble(&parts, offset, zone)
}

/// `parseTimeFormats`: with an offset, without one, minute precision, date.
fn parse_sqlite_formats(text: &str) -> Option<GoTime> {
    for (with_time, with_seconds) in [(true, true), (true, false), (false, false)] {
        let Some(parts) = parse_date_time(text, with_time, with_seconds) else {
            continue;
        };
        if parts.rest.is_empty() {
            return assemble(&parts, 0, "UTC");
        }
        let rest = parts.rest.as_bytes();
        if with_seconds
            && rest.len() == 6
            && matches!(rest[0], b'+' | b'-')
            && rest[3] == b':'
            && let (Ok(hours), Ok(minutes)) = (
                parts.rest[1..3].parse::<i32>(),
                parts.rest[4..6].parse::<i32>(),
            )
            && hours <= 24
            && minutes <= 60
        {
            let mut offset = (hours * 60 + minutes) * 60;
            if rest[0] == b'-' {
                offset = -offset;
            }
            return assemble(&parts, offset, "");
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_round_trip_through_the_driver_rendering() {
        let utc = parse_rfc3339("2026-10-26T01:02:03.5Z").unwrap();
        assert_eq!(utc.to_go_string(), "2026-10-26 01:02:03.5 +0000 UTC");
        assert_eq!(utc.to_rfc3339_nano(), "2026-10-26T01:02:03.5Z");
        let zoned = parse_rfc3339("2026-10-26T03:02:03+02:00").unwrap();
        assert_eq!(zoned.to_go_string(), "2026-10-26 03:02:03 +0200 +0200");
        assert_eq!(zoned.to_rfc3339_nano(), "2026-10-26T03:02:03+02:00");
        assert_eq!(
            zoned.instant(),
            parse_rfc3339("2026-10-26T01:02:03Z").unwrap().instant()
        );
        let back = parse_stored(&zoned.to_go_string()).unwrap();
        assert_eq!(back, zoned.clone());
        assert_eq!(back.to_rfc3339_nano(), "2026-10-26T03:02:03+02:00");
        assert_eq!(
            GoTime::zero().to_go_string(),
            "0001-01-01 00:00:00 +0000 UTC"
        );
    }

    #[test]
    fn go_wording_for_bad_times() {
        assert_eq!(
            parse_rfc3339("bad").unwrap_err(),
            r#"parsing time "bad" as "2006-01-02T15:04:05Z07:00": cannot parse "bad" as "2006""#
        );
        assert_eq!(
            parse_rfc3339("2026-13-26T01:02:03Z").unwrap_err(),
            r#"parsing time "2026-13-26T01:02:03Z": month out of range"#
        );
        assert_eq!(
            parse_rfc3339("2026-10-26T01:02:03").unwrap_err(),
            r#"parsing time "2026-10-26T01:02:03" as "2006-01-02T15:04:05Z07:00": cannot parse "" as "Z07:00""#
        );
        assert_eq!(
            parse_rfc3339("2026-02-30T01:02:03Z").unwrap_err(),
            r#"parsing time "2026-02-30T01:02:03Z": day out of range"#
        );
        assert!(parse_rfc3339("2026-10-26T1:02:03,5+24:00").is_ok());
        assert_eq!(
            unmarshal_json("5").unwrap_err(),
            "Time.UnmarshalJSON: input is not a JSON string"
        );
    }

    #[test]
    fn stored_text_accepts_the_driver_formats() {
        for text in [
            "2026-10-26 01:02:03.123456789 +0000 UTC",
            "2026-10-26T01:02:03Z",
            "2026-10-26 01:02:03",
            "2026-10-26 01:02",
            "2026-10-26",
            "2026-10-26 01:02:03+02:00",
        ] {
            assert!(parse_stored(text).is_some(), "{text}");
        }
        for text in ["", "x", "2026-10-26 01:02:03 +0000", "2026-13-26"] {
            assert!(parse_stored(text).is_none(), "{text}");
        }
    }
}
