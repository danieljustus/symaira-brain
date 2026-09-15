//! Go time.Time JSON deadline parsing for the external decision boundary.
//!
//! Chrono's RFC3339 parser accepts lowercase separators and leap seconds that
//! Go rejects. Parse Go's layout first, retaining its diagnostics and its
//! historical acceptance of one-digit hours, comma fractions and +24:00.
use chrono::{DateTime, FixedOffset, NaiveDate, TimeDelta};

const LAYOUT: &str = "2006-01-02T15:04:05Z07:00";

fn quote(value: &str) -> String {
    symbrain_core::config::format_go_quoted(value.as_ref())
}

struct Parser<'a> {
    value: &'a str,
    rest: &'a str,
}

impl Parser<'_> {
    fn mismatch(&self, layout: &str) -> String {
        format!(
            "parsing time {} as {}: cannot parse {} as {}",
            quote(self.value),
            quote(LAYOUT),
            quote(self.rest),
            quote(layout)
        )
    }

    fn range(&self, name: &str) -> String {
        format!("parsing time {}: {name} out of range", quote(self.value))
    }

    fn literal(&mut self, text: &str) -> Result<(), String> {
        self.rest = self
            .rest
            .strip_prefix(text)
            .ok_or_else(|| self.mismatch(text))?;
        Ok(())
    }

    fn number(&mut self, layout: &str, min: usize, max: usize) -> Result<u32, String> {
        let width = self
            .rest
            .bytes()
            .take(max)
            .take_while(u8::is_ascii_digit)
            .count();
        if width < min {
            return Err(self.mismatch(layout));
        }
        let value = self.rest[..width]
            .parse()
            .map_err(|_| self.mismatch(layout))?;
        self.rest = &self.rest[width..];
        Ok(value)
    }
}

pub(super) fn parse(value: &str) -> Result<DateTime<FixedOffset>, String> {
    let mut p = Parser { value, rest: value };
    let year = p.number("2006", 4, 4)?;
    p.literal("-")?;
    let month = p.number("01", 2, 2)?;
    if !(1..=12).contains(&month) {
        return Err(p.range("month"));
    }
    p.literal("-")?;
    let day = p.number("02", 2, 2)?;
    p.literal("T")?;
    let hour = p.number("15", 1, 2)?;
    if hour > 23 {
        return Err(p.range("hour"));
    }
    p.literal(":")?;
    let minute = p.number("04", 2, 2)?;
    if minute > 59 {
        return Err(p.range("minute"));
    }
    p.literal(":")?;
    let second = p.number("05", 2, 2)?;
    if second > 59 {
        return Err(p.range("second"));
    }
    let mut nanos = 0;
    if matches!(p.rest.as_bytes().first(), Some(b'.' | b','))
        && p.rest.as_bytes().get(1).is_some_and(u8::is_ascii_digit)
    {
        p.rest = &p.rest[1..];
        let width = p.rest.bytes().take_while(u8::is_ascii_digit).count();
        for (i, digit) in p.rest.bytes().take(9).take(width).enumerate() {
            nanos +=
                u32::from(digit - b'0') * 10_u32.pow(8 - u32::try_from(i).expect("nine digits"));
        }
        p.rest = &p.rest[width..];
    }
    let offset = if p.rest.starts_with('Z') {
        p.rest = &p.rest[1..];
        0
    } else {
        let zone = p.rest.as_bytes();
        if zone.len() < 6
            || !matches!(zone[0], b'+' | b'-')
            || zone[3] != b':'
            || ![zone[1], zone[2], zone[4], zone[5]]
                .iter()
                .all(u8::is_ascii_digit)
        {
            return Err(p.mismatch("Z07:00"));
        }
        let hours = i64::from(zone[1] - b'0') * 10 + i64::from(zone[2] - b'0');
        let minutes = i64::from(zone[4] - b'0') * 10 + i64::from(zone[5] - b'0');
        if hours > 24 {
            return Err(p.range("time zone offset hour"));
        }
        if minutes > 60 {
            return Err(p.range("time zone offset minute"));
        }
        let seconds = (hours * 60 + minutes) * 60 * if zone[0] == b'-' { -1 } else { 1 };
        p.rest = &p.rest[6..];
        seconds
    };
    if !p.rest.is_empty() {
        return Err(format!(
            "parsing time {}: extra text: {}",
            quote(value),
            quote(p.rest)
        ));
    }
    let date = NaiveDate::from_ymd_opt(i32::try_from(year).expect("four-digit year"), month, day)
        .ok_or_else(|| p.range("day"))?;
    let local = date
        .and_hms_nano_opt(hour, minute, second, nanos)
        .expect("validated time");
    Ok((local - TimeDelta::seconds(offset))
        .and_utc()
        .fixed_offset())
}
