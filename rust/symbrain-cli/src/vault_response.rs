//! Metadata-only responses with Go's per-byte invalid UTF-8 JSON escaping.
use std::ffi::OsStr;
use std::io::Write;

use serde::Serialize;
use serde_json::value::RawValue;
use symbrain_core::exit;

#[derive(Serialize)]
struct Response<'a> {
    confirmed: Confirmed<'a>,
    submitted: Submitted<'a>,
}

#[derive(Serialize)]
struct Submitted<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    field: Option<&'a str>,
    path: &'a RawValue,
}

#[derive(Serialize)]
struct Confirmed<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    absent: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    field: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    field_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    has_value: Option<bool>,
    path: &'a RawValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    value_matches: Option<bool>,
}

fn path_json(path: &OsStr) -> Box<RawValue> {
    #[cfg(unix)]
    let mut remaining = {
        use std::os::unix::ffi::OsStrExt;
        path.as_bytes()
    };
    #[cfg(not(unix))]
    let display = path.to_string_lossy();
    #[cfg(not(unix))]
    let mut remaining = display.as_bytes();
    let mut encoded = String::from("\"");
    while !remaining.is_empty() {
        let (valid, invalid) = match std::str::from_utf8(remaining) {
            Ok(valid) => (valid, false),
            Err(error) => (
                std::str::from_utf8(&remaining[..error.valid_up_to()]).expect("valid prefix"),
                true,
            ),
        };
        let part = serde_json::to_string(valid).expect("string JSON");
        encoded.push_str(&part[1..part.len() - 1]);
        remaining = &remaining[valid.len()..];
        if invalid {
            // Go advances one byte for every malformed rune and emits a
            // literal JSON escape. Valid U+FFFD remains ordinary UTF-8.
            encoded.push_str("\\ufffd");
            remaining = &remaining[1..];
        }
    }
    encoded.push('"');
    RawValue::from_string(encoded).expect("valid path JSON")
}

pub(super) fn write(
    stdout: &mut dyn Write,
    path: &OsStr,
    confirmed: Option<&str>,
    field: Option<&str>,
    count: Option<usize>,
    absent: bool,
) -> u8 {
    let submitted = path_json(path);
    let confirmed = confirmed.map_or_else(
        || submitted.clone(),
        |path| {
            RawValue::from_string(serde_json::to_string(path).expect("string JSON"))
                .expect("valid confirmed path JSON")
        },
    );
    let result = Response {
        confirmed: Confirmed {
            absent: absent.then_some(true),
            field,
            field_count: count,
            has_value: count.map(|count| count > 0),
            path: &confirmed,
            value_matches: field.map(|_| true),
        },
        submitted: Submitted {
            field,
            path: &submitted,
        },
    };
    let encoded = crate::go_json(&result)
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    if writeln!(stdout, "{encoded}").is_ok() {
        exit::OK
    } else {
        exit::GENERIC
    }
}
