//! Timestamp rendering shared with the shipped memory database.
//!
//! The shipped implementation stores and binds timestamps through its SQLite
//! driver, which renders a `time.Time` as
//! `2006-01-02 15:04:05.999999 +0000 UTC` — microsecond precision with trailing
//! zeros trimmed. String comparisons in the queries (`expires_at > ?`,
//! `ORDER BY created_at`) only agree when both sides use the same rendering,
//! so the native store writes, binds and parses exactly that form.

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};

/// Renders an instant the way the shipped database contains it.
#[must_use]
pub(crate) fn format(time: DateTime<Utc>) -> String {
    let base = time.format("%Y-%m-%d %H:%M:%S").to_string();
    let micros = time.timestamp_subsec_micros();
    if micros == 0 {
        return format!("{base} +0000 UTC");
    }
    let fraction = format!("{micros:06}");
    format!("{base}.{} +0000 UTC", fraction.trim_end_matches('0'))
}

/// Parses every rendering the shipped database can contain.
///
/// Accepts the shipped driver rendering, RFC3339 (rows written by earlier
/// native builds) and a bare `YYYY-MM-DD HH:MM:SS` (SQLite defaults).
#[must_use]
pub(crate) fn parse(raw: &str) -> Option<DateTime<Utc>> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(raw) {
        return Some(parsed.with_timezone(&Utc));
    }
    let mut body = raw.trim();
    for suffix in ["UTC", "+0000"] {
        if let Some(stripped) = body.strip_suffix(suffix) {
            body = stripped.trim_end();
        }
    }
    for layout in [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(body, layout) {
            return Some(Utc.from_utc_datetime(&naive));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{format, parse};

    #[test]
    fn renders_the_shipped_form() {
        let time = parse("2026-09-18 13:19:17.139363 +0000 UTC").unwrap();
        assert_eq!(format(time), "2026-09-18 13:19:17.139363 +0000 UTC");
        // Trailing zeros are trimmed, and a whole second has no fraction.
        let trimmed = parse("2026-09-18 13:19:17.181130 +0000 UTC").unwrap();
        assert_eq!(format(trimmed), "2026-09-18 13:19:17.18113 +0000 UTC");
        let whole = parse("2026-09-18 13:19:17 +0000 UTC").unwrap();
        assert_eq!(format(whole), "2026-09-18 13:19:17 +0000 UTC");
    }

    #[test]
    fn parses_every_known_rendering() {
        for raw in [
            "2026-09-18 13:19:17.139363 +0000 UTC",
            "2026-09-18 13:19:17 +0000 UTC",
            "2026-09-18T13:19:17.139363Z",
            "2026-09-18T13:19:17Z",
            "2026-09-18 13:19:17",
        ] {
            assert!(parse(raw).is_some(), "unparsed: {raw}");
        }
        assert!(parse("not a timestamp").is_none());
    }
}
