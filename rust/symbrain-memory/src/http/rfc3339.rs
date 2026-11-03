//! `time.Parse(time.RFC3339, …)` and `time.RFC3339Nano`: the strict
//! fixed-width form (`T` separator, `Z` or `±hh:mm`), an optional fraction
//! introduced by `.` or `,` and validated calendar ranges.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};

fn digits(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    bytes
        .iter()
        .try_fold(0_u32, |acc, byte| Some(acc * 10 + u32::from(byte - b'0')))
}

pub(super) fn parse(text: &str) -> Option<DateTime<Utc>> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 || !text.is_ascii() {
        return None;
    }
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let year = i32::try_from(digits(&bytes[0..4])?).ok()?;
    let month = digits(&bytes[5..7])?;
    let day = digits(&bytes[8..10])?;
    let hour = digits(&bytes[11..13])?;
    let minute = digits(&bytes[14..16])?;
    let second = digits(&bytes[17..19])?;
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let mut rest = &bytes[19..];
    let mut nanos = 0_u32;
    if matches!(rest.first(), Some(b'.' | b',')) {
        let end = 1 + rest[1..].iter().take_while(|b| b.is_ascii_digit()).count();
        if end == 1 {
            return None;
        }
        let mut value = 0_u32;
        for index in 0..9 {
            value = value * 10
                + rest
                    .get(1 + index)
                    .filter(|_| 1 + index < end)
                    .map_or(0, |b| u32::from(b - b'0'));
        }
        nanos = value;
        rest = &rest[end..];
    }
    let offset = if rest == b"Z" {
        0_i64
    } else if rest.len() == 6 && matches!(rest[0], b'+' | b'-') && rest[3] == b':' {
        let hours = i64::from(digits(&rest[1..3])?);
        let minutes = i64::from(digits(&rest[4..6])?);
        if hours > 23 || minutes > 59 {
            return None;
        }
        let total = hours * 3600 + minutes * 60;
        if rest[0] == b'-' { -total } else { total }
    } else {
        return None;
    };
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let local = date.and_hms_nano_opt(hour, minute, second, nanos)?;
    let utc = local.checked_sub_signed(chrono::Duration::seconds(offset))?;
    Some(Utc.from_utc_datetime(&utc))
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn accepts_the_strict_form_only() {
        assert!(parse("2026-11-03T08:43:55Z").is_some());
        assert!(parse("2026-11-03T08:43:55.123456789Z").is_some());
        assert!(parse("2026-11-03T08:43:55,5+01:00").is_some());
        assert_eq!(
            parse("2026-11-03T09:43:55+01:00"),
            parse("2026-11-03T08:43:55Z")
        );
        for bad in [
            "2026-11-03 08:43:55Z",
            "2026-11-03t08:43:55Z",
            "2026-11-03T08:43:55z",
            "2026-11-03T08:43:55",
            "2026-11-03T24:00:00Z",
            "2026-02-30T00:00:00Z",
            "2026-11-03T08:43:55.Z",
            "2026-11-03T08:43:55+0100",
            "",
        ] {
            assert!(parse(bad).is_none(), "accepted {bad:?}");
        }
    }
}
