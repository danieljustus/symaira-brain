//! JSON and timestamp formatting compatible with Go's `encoding/json`.

use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::ser::{Formatter, Serializer};
use std::io;

/// Serialize a value with Go `encoding/json` string escaping.
///
/// Go escapes `<`, `>`, `&`, U+2028, and U+2029 in addition to the JSON
/// escapes emitted by `serde_json`. Field order and omission remain controlled
/// by the value's serde implementation.
///
/// # Errors
/// Returns the serializer's I/O error.
pub fn to_go_json_vec<T: Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let mut output = Vec::new();
    let formatter = GoFormatter;
    let mut serializer = Serializer::with_formatter(&mut output, formatter);
    value.serialize(&mut serializer)?;
    Ok(output)
}

/// Format a timestamp as Go's UTC `time.RFC3339` layout: seconds precision
/// and a literal `Z`, regardless of the input offset or fractional seconds.
#[must_use]
pub fn format_go_rfc3339(timestamp: DateTime<FixedOffset>) -> String {
    timestamp
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Secs, true)
}

struct GoFormatter;

impl Formatter for GoFormatter {
    fn write_string_fragment<W>(&mut self, writer: &mut W, fragment: &str) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        let mut start = 0;
        for (index, character) in fragment.char_indices() {
            let replacement = match character {
                '<' => Some(br"\u003c".as_slice()),
                '>' => Some(br"\u003e".as_slice()),
                '&' => Some(br"\u0026".as_slice()),
                '\u{2028}' => Some(br"\u2028".as_slice()),
                '\u{2029}' => Some(br"\u2029".as_slice()),
                _ => None,
            };
            if let Some(replacement) = replacement {
                writer.write_all(&fragment.as_bytes()[start..index])?;
                writer.write_all(replacement)?;
                start = index + character.len_utf8();
            }
        }
        writer.write_all(&fragment.as_bytes()[start..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_go_html_and_line_separator_escaping() {
        let value = "<&>\u{2028}\u{2029}";
        assert_eq!(
            to_go_json_vec(&value).unwrap(),
            br#""\u003c\u0026\u003e\u2028\u2029""#
        );
    }

    #[test]
    fn formats_utc_seconds_from_fractional_offset_time() {
        let input: DateTime<FixedOffset> = "2026-09-14T14:00:00.123456789+02:00".parse().unwrap();
        assert_eq!(format_go_rfc3339(input), "2026-09-14T12:00:00Z");
    }
}
