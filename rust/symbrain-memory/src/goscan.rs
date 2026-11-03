//! The shipped `database/sql` + `modernc.org/sqlite` scan semantics.
//!
//! A read in the shipped store ends in `rows.Scan`, which converts the columns
//! of a row in order and stops at the first one that does not convert; the
//! message is `sql: Scan error on column index N, name "col": <reason>`. The
//! driver itself turns `TEXT` values of `DATE`/`DATETIME`/`TIMESTAMP` columns
//! into `time.Time` when one of its layouts parses and delivers the raw string
//! otherwise. This module reproduces both layers so a store with unclean rows
//! reports the shipped diagnostics (and, where Go converts, the same values).

use chrono::{DateTime, FixedOffset, TimeZone};
use rusqlite::Row;
use rusqlite::types::ValueRef;

use crate::slog::quote_bytes;

/// Go-quotes a column name for the scan diagnostic.
fn column(index: usize, name: &str, reason: &str) -> rusqlite::Error {
    go_error(format!(
        "sql: Scan error on column index {index}, name {}: {reason}",
        quote_bytes(name.as_bytes())
    ))
}

/// A shipped diagnostic carried through `rusqlite` row closures; the
/// `From<rusqlite::Error>` for `StoreError` turns it back into its text.
#[derive(Debug)]
pub(crate) struct GoText(pub String);

impl std::fmt::Display for GoText {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for GoText {}

/// Wraps a diagnostic text so it survives a `rusqlite::Result` boundary.
pub(crate) fn go_error(text: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Null, Box::new(GoText(text)))
}

/// The dynamic type name Go prints for a driver value.
fn type_name(value: ValueRef<'_>) -> &'static str {
    match value {
        ValueRef::Null => "<nil>",
        ValueRef::Integer(_) => "int64",
        ValueRef::Real(_) => "float64",
        ValueRef::Text(_) => "string",
        ValueRef::Blob(_) => "[]uint8",
    }
}

/// `strconv.FormatFloat(v, 'g', -1, 64)`.
pub(crate) fn format_g(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "+Inf" } else { "-Inf" }.into();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.into();
    }
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let sign = if negative { "-" } else { "" };
    if !(-4..6).contains(&exponent) {
        // Shortest `%g` switches to exponent form outside [-4, 6).
        let (first, rest) = digits.split_at(1);
        let body = if rest.is_empty() {
            first.to_owned()
        } else {
            format!("{first}.{rest}")
        };
        return format!(
            "{sign}{body}e{}{:02}",
            if exponent < 0 { '-' } else { '+' },
            exponent.abs()
        );
    }
    if exponent >= 0 {
        let point = usize::try_from(exponent).unwrap_or(0) + 1;
        if digits.len() <= point {
            format!("{sign}{digits}{}", "0".repeat(point - digits.len()))
        } else {
            format!("{sign}{}.{}", &digits[..point], &digits[point..])
        }
    } else {
        format!(
            "{sign}0.{}{digits}",
            "0".repeat(usize::try_from(-exponent - 1).unwrap_or(0))
        )
    }
}

/// `database/sql.asString` for a non-null driver value.
fn as_bytes(value: ValueRef<'_>) -> Vec<u8> {
    match value {
        ValueRef::Null => Vec::new(),
        ValueRef::Integer(number) => number.to_string().into_bytes(),
        ValueRef::Real(number) => format_g(number).into_bytes(),
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => bytes.to_vec(),
    }
}

/// First code point of the escape range that carries one byte that is not
/// valid UTF-8 (`U+10FF00 + byte`, plane 16 private use).
const RAW_BASE: u32 = 0x10_FF00;

/// Bytes as a Rust string. A byte that is not valid UTF-8 becomes one private
/// use code point, so the JSON renderer can print the encoder's `\ufffd` and
/// the table renderer can print the original byte back ([`raw_bytes`]).
pub(crate) fn lossy(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        for byte in chunk.invalid() {
            out.extend(char::from_u32(RAW_BASE + u32::from(*byte)));
        }
    }
    out
}

/// Whether `value` carries bytes escaped by [`lossy`].
pub(crate) fn has_raw(value: &str) -> bool {
    value.chars().any(|c| u32::from(c) >= RAW_BASE)
}

/// The stored bytes behind a string produced by [`lossy`].
#[must_use]
pub fn raw_bytes(value: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len());
    let mut buffer = [0_u8; 4];
    for character in value.chars() {
        let code = u32::from(character);
        if code >= RAW_BASE {
            out.push(u8::try_from(code - RAW_BASE).unwrap_or(0));
        } else {
            out.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
        }
    }
    out
}

fn value<'a>(row: &'a Row<'_>, index: usize, name: &str) -> rusqlite::Result<ValueRef<'a>> {
    row.get_ref(index)
        .map_err(|error| go_error(format!("sqlite: {error} (column {name})")))
}

/// A `string` destination: NULL is rejected.
pub(crate) fn text(row: &Row<'_>, index: usize, name: &str) -> rusqlite::Result<String> {
    let value = value(row, index, name)?;
    if matches!(value, ValueRef::Null) {
        return Err(column(index, name, "converting NULL to string is unsupported"));
    }
    Ok(lossy(&as_bytes(value)))
}

/// A `string` destination kept as the stored bytes (JSON columns).
pub(crate) fn raw(row: &Row<'_>, index: usize, name: &str) -> rusqlite::Result<Vec<u8>> {
    let value = value(row, index, name)?;
    if matches!(value, ValueRef::Null) {
        return Err(column(index, name, "converting NULL to string is unsupported"));
    }
    Ok(as_bytes(value))
}

/// `json.Unmarshal(metaStr, &map[string]string)`; the flag says the document
/// was JSON `null` (a nil map, rendered `null` again).
pub(crate) fn decode_metadata(
    bytes: &[u8],
) -> rusqlite::Result<(serde_json::Map<String, serde_json::Value>, bool)> {
    match crate::gojson_decode::decode_string_map(bytes) {
        Ok(Some(map)) => Ok((
            map.into_iter()
                .map(|(key, value)| (key, serde_json::Value::String(value)))
                .collect(),
            false,
        )),
        Ok(None) => Ok((serde_json::Map::new(), true)),
        Err(text) => Err(go_error(text)),
    }
}

/// `json.Unmarshal(embStr, &[]float32)`.
pub(crate) fn decode_embedding(bytes: &[u8]) -> rusqlite::Result<Vec<f32>> {
    crate::gojson_decode::decode_f32_vec(bytes)
        .map(Option::unwrap_or_default)
        .map_err(go_error)
}

/// `json.Unmarshal(aliasesStr, &[]string)`: only the outcome matters.
pub(crate) fn check_string_list(bytes: &[u8]) -> rusqlite::Result<()> {
    crate::gojson_decode::check_valid(bytes).map_err(go_error)?;
    let text = String::from_utf8_lossy(bytes);
    serde_json::from_str::<Option<Vec<Option<String>>>>(&text)
        .map(|_| ())
        .map_err(|_| go_error("json: cannot unmarshal into Go value of type []string".into()))
}

/// A `sql.NullString` destination: NULL is the empty string.
pub(crate) fn null_text(row: &Row<'_>, index: usize, name: &str) -> rusqlite::Result<String> {
    let value = value(row, index, name)?;
    Ok(lossy(&as_bytes(value)))
}

fn parse_error(value: ValueRef<'_>, text: &[u8], target: &str, reason: &str) -> String {
    format!(
        "converting driver.Value type {} ({}) to a {target}: {reason}",
        type_name(value),
        quote_bytes(text)
    )
}

/// `strconv.ParseFloat` over the shapes a stored value can take.
fn parse_float(text: &[u8]) -> Result<f64, &'static str> {
    let Ok(text) = std::str::from_utf8(text) else {
        return Err("invalid syntax");
    };
    let bare = text.trim_start_matches(['+', '-']);
    let is_named = bare.eq_ignore_ascii_case("inf")
        || bare.eq_ignore_ascii_case("infinity")
        || bare.eq_ignore_ascii_case("nan");
    // Rust accepts a few shapes Go rejects (whitespace-free but empty
    // exponent digits) and vice versa for hex floats; the stored values that
    // matter are plain decimals.
    if text.is_empty()
        || text.starts_with(char::is_whitespace)
        || text.ends_with(char::is_whitespace)
        || text.matches(['+', '-']).count() > 2
    {
        return Err("invalid syntax");
    }
    match text.parse::<f64>() {
        Ok(number) if number.is_infinite() && !is_named => Err("value out of range"),
        Ok(number) => Ok(number),
        Err(_) => Err("invalid syntax"),
    }
}

/// A `float64` destination.
pub(crate) fn float(row: &Row<'_>, index: usize, name: &str) -> rusqlite::Result<f64> {
    let value = value(row, index, name)?;
    match value {
        ValueRef::Null => Err(column(index, name, "converting NULL to float64 is unsupported")),
        ValueRef::Real(number) => Ok(number),
        other => {
            let text = as_bytes(other);
            parse_float(&text).map_err(|reason| {
                column(index, name, &parse_error(other, &text, "float64", reason))
            })
        }
    }
}

fn parse_int(text: &[u8]) -> Result<i64, &'static str> {
    let Ok(text) = std::str::from_utf8(text) else {
        return Err("invalid syntax");
    };
    match text.parse::<i64>() {
        Ok(number) => Ok(number),
        Err(error) => match error.kind() {
            std::num::IntErrorKind::PosOverflow | std::num::IntErrorKind::NegOverflow => {
                Err("value out of range")
            }
            _ => Err("invalid syntax"),
        },
    }
}

/// An `int64` destination.
pub(crate) fn int(row: &Row<'_>, index: usize, name: &str) -> rusqlite::Result<i64> {
    let value = value(row, index, name)?;
    match value {
        ValueRef::Null => Err(column(index, name, "converting NULL to int64 is unsupported")),
        ValueRef::Integer(number) => Ok(number),
        other => {
            let text = as_bytes(other);
            parse_int(&text).map_err(|reason| {
                column(index, name, &parse_error(other, &text, "int64", reason))
            })
        }
    }
}

/// A `sql.NullInt64` destination.
pub(crate) fn null_int(
    row: &Row<'_>,
    index: usize,
    name: &str,
) -> rusqlite::Result<Option<i64>> {
    if matches!(value(row, index, name)?, ValueRef::Null) {
        return Ok(None);
    }
    int(row, index, name).map(Some)
}

/// A `[]byte` destination (`NULL` is a nil slice).
pub(crate) fn blob(
    row: &Row<'_>,
    index: usize,
    name: &str,
) -> rusqlite::Result<Option<Vec<u8>>> {
    match value(row, index, name)? {
        ValueRef::Null => Ok(None),
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => Ok(Some(bytes.to_vec())),
        other => Err(column(
            index,
            name,
            &format!(
                "unsupported Scan, storing driver.Value type {} into type *[]uint8",
                type_name(other)
            ),
        )),
    }
}

fn time_value(value: ValueRef<'_>) -> Result<DateTime<FixedOffset>, String> {
    match value {
        ValueRef::Text(bytes) => std::str::from_utf8(bytes)
            .ok()
            .and_then(driver_time)
            .ok_or_else(|| {
                "unsupported Scan, storing driver.Value type string into type *time.Time".into()
            }),
        other => Err(format!(
            "unsupported Scan, storing driver.Value type {} into type *time.Time",
            type_name(other)
        )),
    }
}

/// A `time.Time` destination.
pub(crate) fn time(row: &Row<'_>, index: usize, name: &str) -> rusqlite::Result<DateTime<FixedOffset>> {
    time_value(value(row, index, name)?).map_err(|reason| column(index, name, &reason))
}

/// A `sql.NullTime` destination.
pub(crate) fn null_time(
    row: &Row<'_>,
    index: usize,
    name: &str,
) -> rusqlite::Result<Option<DateTime<FixedOffset>>> {
    let value = value(row, index, name)?;
    if matches!(value, ValueRef::Null) {
        return Ok(None);
    }
    time_value(value)
        .map(Some)
        .map_err(|reason| column(index, name, &reason))
}

// --- The driver's time parser -------------------------------------------

struct Cursor<'a> {
    rest: &'a [u8],
}

impl Cursor<'_> {
    /// `getnum`: two digits when `fixed`, else one or two.
    fn number(&mut self, fixed: bool) -> Option<i64> {
        let first = self.rest.first().filter(|b| b.is_ascii_digit())?;
        if let Some(second) = self.rest.get(1).filter(|b| b.is_ascii_digit()) {
            self.rest = &self.rest[2..];
            return Some(i64::from(first - b'0') * 10 + i64::from(second - b'0'));
        }
        if fixed {
            return None;
        }
        self.rest = &self.rest[1..];
        Some(i64::from(first - b'0'))
    }

    fn literal(&mut self, byte: u8) -> Option<()> {
        (self.rest.first() == Some(&byte)).then(|| self.rest = &self.rest[1..])
    }

    fn year(&mut self) -> Option<i64> {
        let head = self.rest.get(..4)?;
        if !head.iter().all(u8::is_ascii_digit) {
            return None;
        }
        self.rest = &self.rest[4..];
        std::str::from_utf8(head).ok()?.parse().ok()
    }

    /// Fractional seconds introduced by `.` or `,`; `None` leaves the value.
    fn fraction(&mut self) -> u32 {
        let digits = match self.rest {
            [b'.' | b',', first, ..] if first.is_ascii_digit() => {
                self.rest[1..].iter().take_while(|b| b.is_ascii_digit()).count()
            }
            _ => return 0,
        };
        let taken = &self.rest[1..=digits];
        self.rest = &self.rest[1 + digits..];
        let mut nanos = 0_u32;
        for index in 0..9 {
            nanos = nanos * 10 + taken.get(index).map_or(0, |b| u32::from(b - b'0'));
        }
        nanos
    }
}

fn days_in(month: i64, year: i64) -> i64 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Layout flags of the driver's `parseTimeFormats`.
struct Layout {
    separator: u8,
    seconds: bool,
    zone: bool,
    time: bool,
}

const LAYOUTS: [Layout; 7] = [
    Layout { separator: b' ', seconds: true, zone: true, time: true },
    Layout { separator: b'T', seconds: true, zone: true, time: true },
    Layout { separator: b' ', seconds: true, zone: false, time: true },
    Layout { separator: b'T', seconds: true, zone: false, time: true },
    Layout { separator: b' ', seconds: false, zone: false, time: true },
    Layout { separator: b'T', seconds: false, zone: false, time: true },
    Layout { separator: b' ', seconds: false, zone: false, time: false },
];

/// `time.Parse` for one driver layout.
fn parse_layout(text: &str, layout: &Layout) -> Option<DateTime<FixedOffset>> {
    let mut cursor = Cursor { rest: text.as_bytes() };
    let year = cursor.year()?;
    cursor.literal(b'-')?;
    let month = cursor.number(true)?;
    cursor.literal(b'-')?;
    let day = cursor.number(true)?;
    if !(1..=12).contains(&month) || day < 1 || day > days_in(month, year) {
        return None;
    }
    let (mut hour, mut minute, mut second, mut nanos, mut offset) = (0, 0, 0, 0, 0_i64);
    if layout.time {
        cursor.literal(layout.separator)?;
        hour = cursor.number(false)?;
        cursor.literal(b':')?;
        minute = cursor.number(true)?;
        if layout.seconds {
            cursor.literal(b':')?;
            second = cursor.number(true)?;
        }
        if layout.seconds {
            nanos = cursor.fraction();
        }
        if hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        if layout.zone {
            let sign = match cursor.rest.first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            cursor.rest = &cursor.rest[1..];
            let offset_hour = cursor.number(true)?;
            cursor.literal(b':')?;
            let offset_minute = cursor.number(true)?;
            if offset_hour > 24 || offset_minute > 60 {
                return None;
            }
            offset = sign * (offset_hour * 3600 + offset_minute * 60);
        }
    }
    if !cursor.rest.is_empty() {
        return None;
    }
    let date = chrono::NaiveDate::from_ymd_opt(i32::try_from(year).ok()?, u32::try_from(month).ok()?, u32::try_from(day).ok()?)?;
    let naive = date.and_hms_nano_opt(
        u32::try_from(hour).ok()?,
        u32::try_from(minute).ok()?,
        u32::try_from(second).ok()?,
        nanos,
    )?;
    FixedOffset::east_opt(i32::try_from(offset).ok()?)?
        .from_local_datetime(&naive)
        .single()
}

/// `time.Parse("2006-01-02 15:04:05.999999999 -0700 MST", s)` after the
/// driver's monotonic-clock trim.
fn parse_string_form(raw: &str) -> Option<DateTime<FixedOffset>> {
    let mut text = raw;
    if let Some(position) = raw.find("m=").filter(|position| *position > 0) {
        text = &raw[..position];
    }
    let text = text.trim_matches(|c: char| c.is_whitespace());
    let mut cursor = Cursor { rest: text.as_bytes() };
    let year = cursor.year()?;
    cursor.literal(b'-')?;
    let month = cursor.number(true)?;
    cursor.literal(b'-')?;
    let day = cursor.number(true)?;
    cursor.literal(b' ')?;
    let hour = cursor.number(false)?;
    cursor.literal(b':')?;
    let minute = cursor.number(true)?;
    cursor.literal(b':')?;
    let second = cursor.number(true)?;
    let nanos = cursor.fraction();
    cursor.literal(b' ')?;
    let sign = match cursor.rest.first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    cursor.rest = &cursor.rest[1..];
    let offset_hour = cursor.number(true)?;
    let offset_minute = cursor.number(true)?;
    cursor.literal(b' ')?;
    let zone = cursor.rest;
    let letters = zone.iter().take_while(|b| b.is_ascii_uppercase()).count();
    if !(3..=4).contains(&letters) || letters != zone.len() {
        // Go also accepts `GMT+3` style names and `+hh`; the stored form is
        // always the three-letter abbreviation.
        return None;
    }
    if !(1..=12).contains(&month)
        || day < 1
        || day > days_in(month, year)
        || hour > 23
        || minute > 59
        || second > 59
        || offset_hour > 24
        || offset_minute > 60
    {
        return None;
    }
    let date = chrono::NaiveDate::from_ymd_opt(
        i32::try_from(year).ok()?,
        u32::try_from(month).ok()?,
        u32::try_from(day).ok()?,
    )?;
    let naive = date.and_hms_nano_opt(
        u32::try_from(hour).ok()?,
        u32::try_from(minute).ok()?,
        u32::try_from(second).ok()?,
        nanos,
    )?;
    let offset = sign * (offset_hour * 3600 + offset_minute * 60);
    FixedOffset::east_opt(i32::try_from(offset).ok()?)?
        .from_local_datetime(&naive)
        .single()
}

/// `(*conn).parseTime`: the value a `DATETIME` column's text becomes, or
/// `None` when the driver leaves it a string.
pub(crate) fn driver_time(raw: &str) -> Option<DateTime<FixedOffset>> {
    if let Some(parsed) = parse_string_form(raw) {
        return Some(parsed);
    }
    let text = raw.strip_suffix('Z').unwrap_or(raw);
    LAYOUTS.iter().find_map(|layout| parse_layout(text, layout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_floats_like_strconv_g() {
        for (value, expected) in [
            (0.5, "0.5"),
            (1.0, "1"),
            (100_000.0, "100000"),
            (1_234_567.0, "1.234567e+06"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (-2.5, "-2.5"),
            (1e21, "1e+21"),
            (123_456_789.0, "1.23456789e+08"),
        ] {
            assert_eq!(format_g(value), expected, "{value}");
        }
    }

    #[test]
    fn driver_time_accepts_the_driver_layouts_only() {
        for good in [
            "2026-09-18 13:19:17.139363 +0000 UTC",
            "2026-09-18 13:19:17 +0000 UTC m=+1.5",
            "2026-09-18T13:19:17.139363Z",
            "2026-09-18 13:19:17+02:00",
            "2026-09-18 13:19:17",
            "2026-09-18 13:19",
            "2026-09-18T13:19",
            "2026-09-18",
            "2026-09-18 5:19",
        ] {
            assert!(driver_time(good).is_some(), "{good}");
        }
        for bad in [
            "",
            "invalid",
            "2026-09-31",
            "2026-09-18 24:00:00",
            "2026-09-18 13:19:60",
            "2026-09-18 13:19:17 extra",
            "2026-9-18",
            "2026-09-18 13:19:17Z+01:00",
        ] {
            assert!(driver_time(bad).is_none(), "{bad}");
        }
    }
}
