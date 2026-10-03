//! Go-compatible quoting keeps rejected session IDs stable and unambiguous.
use std::fmt::Write;

pub(crate) fn quote(value: &str) -> String {
    quote_bytes(value.as_bytes())
}

/// Formats the socket-validation diagnostic without discarding Unix argv bytes.
#[must_use]
pub fn invalid_session_message(value: &[u8]) -> String {
    format!(
        "invalid session {}: use 1-64 letters, digits, '.', '_' or '-'",
        quote_bytes(value)
    )
}

fn quote_bytes(mut value: &[u8]) -> String {
    let mut result = String::from("\"");
    while !value.is_empty() {
        match std::str::from_utf8(value) {
            Ok(text) => {
                append_text(&mut result, text);
                break;
            }
            Err(error) => {
                let (valid, rest) = value.split_at(error.valid_up_to());
                append_text(
                    &mut result,
                    std::str::from_utf8(valid).expect("validated prefix"),
                );
                // Go's DecodeRune consumes one byte for malformed UTF-8.
                write!(result, "\\x{:02x}", rest[0]).expect("writing to a String cannot fail");
                value = &rest[1..];
            }
        }
    }
    result.push('"');
    result
}

fn append_text(result: &mut String, value: &str) {
    for ch in value.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\u{7}' => result.push_str("\\a"),
            '\u{8}' => result.push_str("\\b"),
            '\u{c}' => result.push_str("\\f"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\u{b}' => result.push_str("\\v"),
            ch => {
                let code = u32::from(ch);
                let ranges = crate::go_print::NON_PRINTABLE;
                let index = ranges.partition_point(|(_, end)| *end < code);
                if ranges.get(index).is_none_or(|(start, _)| code < *start) {
                    result.push(ch);
                } else if code < 0x20 || code == 0x7f {
                    write!(result, "\\x{code:02x}").expect("writing to a String cannot fail");
                } else if code < 0x10000 {
                    write!(result, "\\u{code:04x}").expect("writing to a String cannot fail");
                } else {
                    write!(result, "\\U{code:08x}").expect("writing to a String cannot fail");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn malformed_bytes_remain_distinct_from_real_replacement_scalars() {
        for (value, expected) in [
            (b"bad\xffsession".as_slice(), "\"bad\\xffsession\""),
            (b"bad\xe2\x82session".as_slice(), "\"bad\\xe2\\x82session\""),
            (b"bad\xc0\xafsession".as_slice(), "\"bad\\xc0\\xafsession\""),
            ("bad\u{fffd}session".as_bytes(), "\"bad\u{fffd}session\""),
        ] {
            assert_eq!(quote_bytes(value), expected);
        }
    }

    #[test]
    fn every_valid_unicode_scalar_matches_actual_pinned_go_quote_digest() {
        // Actual Go 1.26.7 strconv.AppendQuote for each valid scalar, each
        // followed by newline; the producing probe is retained in evidence.
        let mut digest = Sha256::new();
        for code in 0..=0x10ffff {
            if let Some(ch) = char::from_u32(code) {
                digest.update(quote(&ch.to_string()).as_bytes());
                digest.update(b"\n");
            }
        }
        assert_eq!(
            format!("{:x}", digest.finalize()),
            "4c752b4c6e90df8c641d6ac02a6da80113943bc8fc476c825f27aef51db2a055"
        );
    }
}
