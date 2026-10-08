//! Go-compatible quoting keeps rejected session IDs stable and unambiguous.
use std::fmt::Write;

pub fn quote(value: &str) -> String {
    quote_bytes(value.as_bytes())
}

pub fn quote_bytes(mut value: &[u8]) -> String {
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
                let ranges = super::go_print::NON_PRINTABLE;
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
