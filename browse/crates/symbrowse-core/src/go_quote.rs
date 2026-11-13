//! Go-compatible quoting keeps rejected session IDs stable and unambiguous.
use std::fmt::Write;

pub fn quote(value: &str) -> String {
    quote_bytes(value.as_bytes())
}

/// First scalar of the reserved block that carries malformed argv bytes.
///
/// Go keeps command-line arguments as raw bytes and its `%q` prints a byte
/// that is not valid UTF-8 as `\xNN`. The Rust command tree works on `String`s,
/// so [`argv_text`] maps each malformed byte `0x80..=0xFF` to the unassigned
/// private-use scalar `ARGV_BYTE_BASE + byte` and [`quote_argv`] maps it back.
/// Only argv-derived text may go through [`quote_argv`]; [`quote`] never
/// reinterprets scalars, so genuine text keeps its exact Go quoting.
const ARGV_BYTE_BASE: u32 = 0x10_F700;

/// Converts one raw argument into text without losing malformed bytes.
///
/// Valid UTF-8 is returned unchanged. On Unix each malformed byte becomes its
/// reserved scalar so [`quote_argv`] reproduces Go's `\xNN`; elsewhere arguments
/// are already Unicode and conversion is lossy.
#[must_use]
pub fn argv_text(value: &std::ffi::OsStr) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut text = String::new();
        for chunk in value.as_bytes().utf8_chunks() {
            text.push_str(chunk.valid());
            for byte in chunk.invalid() {
                text.push(
                    char::from_u32(ARGV_BYTE_BASE + u32::from(*byte)).expect("reserved scalar"),
                );
            }
        }
        text
    }
    #[cfg(not(unix))]
    {
        value.to_string_lossy().into_owned()
    }
}

fn argv_bytes(value: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(value.len());
    for ch in value.chars() {
        match u32::from(ch).checked_sub(ARGV_BYTE_BASE) {
            Some(byte @ 0x80..=0xff) => bytes.push(u8::try_from(byte).expect("byte range")),
            _ => bytes.extend_from_slice(ch.to_string().as_bytes()),
        }
    }
    bytes
}

/// Go `%q` of an argument produced by [`argv_text`].
#[must_use]
pub fn quote_argv(value: &str) -> String {
    quote_bytes(&argv_bytes(value))
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    #[test]
    fn malformed_argv_bytes_quote_like_go_after_the_string_carrier() {
        for (raw, expected) in [
            (b"bad\xffsession".as_slice(), "\"bad\\xffsession\""),
            (b"bad\xe2\x82session".as_slice(), "\"bad\\xe2\\x82session\""),
            (b"bad\xc0\xafsession".as_slice(), "\"bad\\xc0\\xafsession\""),
            (b"ok \xe2\x82\xac \xff".as_slice(), "\"ok \u{20ac} \\xff\""),
        ] {
            let text = argv_text(&OsString::from_vec(raw.to_vec()));
            assert_eq!(quote_argv(&text), expected);
            assert_eq!(quote_bytes(raw), expected);
        }
    }

    #[test]
    fn plain_quote_never_reinterprets_reserved_scalars() {
        let reserved = "a\u{10f7ff}b";
        assert_eq!(quote(reserved), "\"a\\U0010f7ffb\"");
        assert_eq!(quote_argv(reserved), "\"a\\xffb\"");
    }

    #[test]
    fn valid_unicode_including_replacement_scalars_is_unchanged() {
        for value in ["plain", "bad\u{fffd}session", "caf\u{e9}", "\u{10f600}"] {
            assert_eq!(argv_text(std::ffi::OsStr::new(value)), value);
            assert_eq!(quote(value), quote_bytes(value.as_bytes()));
            assert_eq!(quote_argv(value), quote(value));
        }
    }
}
