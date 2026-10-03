//! Go-compatible quoting keeps rejected session IDs stable and unambiguous.
use std::fmt::Write;

pub(crate) fn quote(value: &str) -> String {
    let mut result = String::from("\"");
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
    result.push('"');
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

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
