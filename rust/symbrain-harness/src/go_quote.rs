//! Go `strconv` quoting helpers shared by the Go-parity JSON and TOML decoders.

use symbrain_core::config::format_go_quoted_bytes;

/// `encoding/json`'s `quoteChar`: a Go byte as a single-quoted literal.
pub(crate) fn json_quote_char(byte: u8) -> String {
    match byte {
        b'\'' => "'\\''".to_owned(),
        b'"' => "'\"'".to_owned(),
        _ => {
            let quoted = format_go_quoted_bytes(char::from(byte).to_string().as_bytes());
            format!("'{}'", &quoted[1..quoted.len() - 1])
        }
    }
}

/// Go's `%q` for a rune (`strconv.QuoteRune`).
pub(crate) fn quote_rune(rune: char) -> String {
    let quoted = format_go_quoted_bytes(rune.to_string().as_bytes());
    let inner = &quoted[1..quoted.len() - 1];
    match rune {
        '\'' => "'\\''".to_owned(),
        '"' => "'\"'".to_owned(),
        _ => format!("'{inner}'"),
    }
}

/// Go's `%q` for a string.
pub(crate) fn quote_str(value: &str) -> String {
    format_go_quoted_bytes(value.as_bytes())
}

/// Go's `utf8.DecodeRune`: invalid or truncated input yields `(U+FFFD, 1)`.
pub(crate) fn decode_rune(bytes: &[u8]) -> (char, usize) {
    let Some(&first) = bytes.first() else {
        return (char::REPLACEMENT_CHARACTER, 0);
    };
    if first < 0x80 {
        return (char::from(first), 1);
    }
    let width = match first {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => return (char::REPLACEMENT_CHARACTER, 1),
    };
    bytes
        .get(..width)
        .and_then(|slice| std::str::from_utf8(slice).ok())
        .and_then(|text| text.chars().next())
        .map_or((char::REPLACEMENT_CHARACTER, 1), |ch| (ch, width))
}
