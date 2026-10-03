//! `RFC3339Nano` parsing and diagnostics from the frozen Go 1.26.7 contract.
use chrono::{DateTime, NaiveDate, Utc};
use std::fmt::Write;
const LAYOUT: &str = "2006-01-02T15:04:05.999999999Z07:00";

fn quote(value: &[u8]) -> String {
    let mut out = String::from("\"");
    for &byte in value {
        match byte {
            b'"' | b'\\' => {
                out.push('\\');
                out.push(char::from(byte));
            }
            0..=31 | 128..=255 => {
                let _ = write!(out, "\\x{byte:02x}");
            }
            _ => out.push(char::from(byte)),
        }
    }
    out.push('"');
    out
}
struct Parser<'a> {
    original: &'a [u8],
    rest: &'a [u8],
}
impl Parser<'_> {
    fn invalid(&self, element: &str) -> String {
        format!(
            "parsing time {} as {}: cannot parse {} as {}",
            quote(self.original),
            quote(LAYOUT.as_bytes()),
            quote(self.rest),
            quote(element.as_bytes())
        )
    }
    fn range(&self, name: &str) -> String {
        format!("parsing time {}: {name} out of range", quote(self.original))
    }
    fn literal(&mut self, text: &str) -> Result<(), String> {
        if !self.rest.starts_with(text.as_bytes()) {
            return Err(self.invalid(text));
        }
        self.rest = &self.rest[text.len()..];
        Ok(())
    }
    fn number(&mut self, element: &str, width: usize, flexible: bool) -> Result<u32, String> {
        let count = self
            .rest
            .iter()
            .take(width)
            .take_while(|b| b.is_ascii_digit())
            .count();
        if count == 0 || (!flexible && count != width) {
            return Err(self.invalid(element));
        }
        let number = self.rest[..count]
            .iter()
            .fold(0, |n, b| n * 10 + u32::from(b - b'0'));
        self.rest = &self.rest[count..];
        Ok(number)
    }
}
pub(super) fn parse(value: &[u8]) -> Result<DateTime<Utc>, String> {
    let mut p = Parser {
        original: value,
        rest: value,
    };
    let year = p.number("2006", 4, false)?;
    p.literal("-")?;
    let month = p.number("01", 2, false)?;
    if !(1..=12).contains(&month) {
        return Err(p.range("month"));
    }
    p.literal("-")?;
    let day = p.number("02", 2, false)?;
    p.literal("T")?;
    let hour = p.number("15", 2, true)?;
    if hour >= 24 {
        return Err(p.range("hour"));
    }
    p.literal(":")?;
    let minute = p.number("04", 2, false)?;
    if minute >= 60 {
        return Err(p.range("minute"));
    }
    p.literal(":")?;
    let second = p.number("05", 2, false)?;
    if second >= 60 {
        return Err(p.range("second"));
    }
    let mut nanos = 0;
    if matches!(p.rest.first(), Some(b'.' | b',')) && p.rest.get(1).is_some_and(u8::is_ascii_digit)
    {
        p.rest = &p.rest[1..];
        let count = p.rest.iter().take_while(|b| b.is_ascii_digit()).count();
        for i in 0..9 {
            nanos = nanos * 10
                + p.rest
                    .get(i)
                    .filter(|_| i < count)
                    .map_or(0, |b| u32::from(b - b'0'));
        }
        p.rest = &p.rest[count..];
    }
    let offset = if p.rest.starts_with(b"Z") {
        p.rest = &p.rest[1..];
        0
    } else {
        let saved = p.rest;
        if saved.len() < 6 || saved[3] != b':' {
            return Err(p.invalid("Z07:00"));
        }
        let valid_hours = saved[1..3].iter().all(u8::is_ascii_digit);
        let valid_minutes = valid_hours && saved[4..6].iter().all(u8::is_ascii_digit);
        let hours = if valid_hours {
            i64::from(saved[1] - b'0') * 10 + i64::from(saved[2] - b'0')
        } else {
            0
        };
        let minutes = if valid_minutes {
            i64::from(saved[4] - b'0') * 10 + i64::from(saved[5] - b'0')
        } else {
            0
        };
        if minutes > 60 {
            return Err(p.range("time zone offset minute"));
        }
        if hours > 24 {
            return Err(p.range("time zone offset hour"));
        }
        if !valid_hours || !valid_minutes || !matches!(saved[0], b'+' | b'-') {
            return Err(p.invalid("Z07:00"));
        }
        p.rest = &saved[6..];
        (hours * 3600 + minutes * 60) * if saved[0] == b'-' { -1 } else { 1 }
    };
    if !p.rest.is_empty() {
        return Err(format!(
            "parsing time {}: extra text: {}",
            quote(p.original),
            quote(p.rest)
        ));
    }
    let date = NaiveDate::from_ymd_opt(i32::try_from(year).expect("four-digit year"), month, day)
        .ok_or_else(|| p.range("day"))?;
    let local = date
        .and_hms_nano_opt(hour, minute, second, nanos)
        .expect("validated time")
        .and_utc();
    Ok(local - chrono::Duration::seconds(offset))
}
