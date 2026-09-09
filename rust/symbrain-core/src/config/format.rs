//! Go-compatible formatting for values returned by `config get`.

use std::fmt::Write as _;
use toml_edit::{Array, ArrayOfTables, InlineTable, Item, Table, Value};

#[path = "../go_printable.rs"]
mod go_printable;

const GO_QUOTE_HEX: &[u8; 16] = b"0123456789abcdef";

/// Quotes raw bytes with Go's byte-preserving `%q` behavior.
pub(super) fn format_go_quoted_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 2);
    let mut remaining = bytes;
    out.push('"');
    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                append_go_quoted_str(&mut out, valid);
                break;
            }
            Err(error) => {
                let valid_len = error.valid_up_to();
                if valid_len > 0 {
                    append_go_quoted_str(
                        &mut out,
                        std::str::from_utf8(&remaining[..valid_len]).expect("valid prefix"),
                    );
                }
                append_go_hex_escape(&mut out, remaining[valid_len]);
                remaining = &remaining[valid_len + 1..];
            }
        }
    }
    out.push('"');
    out
}

/// Quotes an argument with Go's byte-preserving `%q` behavior.
#[cfg_attr(not(test), allow(dead_code))]
#[must_use]
pub fn format_go_quoted(arg: &std::ffi::OsStr) -> String {
    format_go_quoted_bytes(&crate::config::set::os_bytes(arg))
}

fn append_go_quoted_str(out: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\x07' => out.push_str("\\a"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x0b' => out.push_str("\\v"),
            ch if go_printable::is_printable(ch) => out.push(ch),
            ch if ch.is_ascii() => append_go_hex_escape(out, ch as u8),
            ch if (ch as u32) < 0x10000 => {
                let _ = write!(out, "\\u{:04x}", ch as u32);
            }
            ch => {
                let _ = write!(out, "\\U{:08x}", ch as u32);
            }
        }
    }
}

fn append_go_hex_escape(out: &mut String, byte: u8) {
    out.push_str("\\x");
    out.push(GO_QUOTE_HEX[(byte >> 4) as usize] as char);
    out.push(GO_QUOTE_HEX[(byte & 0x0f) as usize] as char);
}

pub(super) fn format_go_float_scientific(f: f64) -> Option<String> {
    let abs = f.abs();
    if abs < 1e-4 || abs >= 1e6 {
        let s = format!("{f:e}");
        if let Some((mantissa, exp)) = s.split_once('e') {
            let exp_num: i32 = exp.parse().unwrap_or(0);
            return Some(format!("{mantissa}e{exp_num:+03}"));
        }
    }
    None
}

fn format_go_float(f: f64, w: &mut String) {
    if f.is_nan() {
        w.push_str("NaN");
    } else if f.is_infinite() {
        w.push_str(if f.is_sign_positive() { "+Inf" } else { "-Inf" });
    } else if f == 0.0 {
        w.push_str(if f.is_sign_negative() { "-0" } else { "0" });
    } else if let Some(sci) = format_go_float_scientific(f) {
        w.push_str(&sci);
    } else if f.fract() == 0.0 {
        let _ = write!(w, "{f:.0}");
    } else {
        w.push_str(&f.to_string());
    }
}

fn local_offset_seconds() -> i32 {
    use chrono::Offset as _;
    static OFFSET: std::sync::OnceLock<i32> = std::sync::OnceLock::new();
    *OFFSET.get_or_init(|| chrono::Local::now().offset().fix().local_minus_utc())
}

fn format_offset_hhmm(seconds: i32) -> String {
    let sign = if seconds >= 0 { '+' } else { '-' };
    let abs = seconds.unsigned_abs();
    format!("{sign}{:02}{:02}", abs / 3600, (abs % 3600) / 60)
}

pub(super) fn format_date_str(d: toml_edit::Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year, d.month, d.day)
}

pub(super) fn format_time_str(t: &toml_edit::Time) -> String {
    let base = format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second.unwrap_or(0));
    let frac = t
        .nanosecond
        .filter(|n| *n > 0)
        .map_or_else(String::new, |n| {
            format!(".{}", format!("{n:09}").trim_end_matches('0'))
        });
    format!("{base}{frac}")
}

fn format_datetime_with_offset(dt: &toml_edit::Datetime, local: i32, w: &mut String) {
    let date = dt
        .date
        .map_or_else(|| "0000-01-01".to_owned(), format_date_str);
    let time = dt
        .time
        .as_ref()
        .map_or_else(|| "00:00:00".to_owned(), format_time_str);
    let zone = match dt.offset {
        Some(toml_edit::Offset::Z) => "+0000 UTC".to_owned(),
        Some(toml_edit::Offset::Custom { minutes }) => {
            let off = format_offset_hhmm(i32::from(minutes) * 60);
            if minutes == 0 && local == 0 {
                "+0000 UTC".to_owned()
            } else {
                format!("{off} {off}")
            }
        }
        None => {
            let off = format_offset_hhmm(local);
            if dt.date.is_some() && dt.time.is_some() {
                format!("{off} datetime-local")
            } else if dt.date.is_some() {
                format!("{off} date-local")
            } else {
                format!("{off} time-local")
            }
        }
    };
    let _ = write!(w, "{date} {time} {zone}");
}

fn format_datetime(dt: &toml_edit::Datetime, w: &mut String) {
    format_datetime_with_offset(dt, local_offset_seconds(), w);
}

#[cfg(test)]
pub(super) fn format_go_datetime_with_offset(dt: &toml_edit::Datetime, local: i32, w: &mut String) {
    format_datetime_with_offset(dt, local, w);
}

fn format_value(value: &Value, w: &mut String) {
    match value {
        Value::String(s) => w.push_str(s.value()),
        Value::Integer(i) => {
            let _ = write!(w, "{}", i.value());
        }
        Value::Boolean(b) => w.push_str(if *b.value() { "true" } else { "false" }),
        Value::Float(f) => format_go_float(*f.value(), w),
        Value::Datetime(dt) => format_datetime(dt.value(), w),
        Value::Array(a) => format_array(a, w),
        Value::InlineTable(t) => format_inline_table(t, w),
    }
}

fn format_array(array: &Array, w: &mut String) {
    w.push('[');
    for (i, value) in array.iter().enumerate() {
        if i > 0 {
            w.push(' ');
        }
        format_value(value, w);
    }
    w.push(']');
}

fn format_inline_table(table: &InlineTable, w: &mut String) {
    w.push_str("map[");
    let mut entries: Vec<_> = table.iter().collect();
    entries.sort_by_key(|(key, _)| *key);
    for (i, (key, value)) in entries.iter().enumerate() {
        if i > 0 {
            w.push(' ');
        }
        w.push_str(key);
        w.push(':');
        format_value(value, w);
    }
    w.push(']');
}

fn format_table(table: &Table, w: &mut String) {
    w.push_str("map[");
    let mut entries: Vec<_> = table.iter().collect();
    entries.sort_by_key(|(key, _)| *key);
    for (i, (key, item)) in entries.iter().enumerate() {
        if i > 0 {
            w.push(' ');
        }
        w.push_str(key);
        w.push(':');
        format_item(item, w);
    }
    w.push(']');
}

fn format_item(item: &Item, w: &mut String) {
    match item {
        Item::Table(t) => format_table(t, w),
        Item::Value(v) => format_value(v, w),
        Item::ArrayOfTables(a) => {
            w.push('[');
            for (i, t) in a.iter().enumerate() {
                if i > 0 {
                    w.push(' ');
                }
                format_table(t, w);
            }
            w.push(']');
        }
        Item::None => {}
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum TomlRef<'a> {
    Table(&'a Table),
    InlineTable(&'a InlineTable),
    ArrayOfTables(&'a ArrayOfTables),
    Value(&'a Value),
}

pub(super) fn format_toml_ref(node: TomlRef<'_>, w: &mut String) {
    match node {
        TomlRef::Table(t) => format_table(t, w),
        TomlRef::InlineTable(t) => format_inline_table(t, w),
        TomlRef::ArrayOfTables(a) => {
            w.push('[');
            for (i, t) in a.iter().enumerate() {
                if i > 0 {
                    w.push(' ');
                }
                format_table(t, w);
            }
            w.push(']');
        }
        TomlRef::Value(v) => format_value(v, w),
    }
}

#[cfg(test)]
#[path = "format_tests.rs"]
mod tests;
