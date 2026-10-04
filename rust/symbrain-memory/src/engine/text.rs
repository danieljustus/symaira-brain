//! Go rune/string behavior required by the engine's byte-valued algorithms.

pub(super) fn whitespace(c: char) -> bool {
    matches!(c, '\t'..='\r' | ' ' | '\u{85}' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}

pub(super) fn trim(text: &str) -> &str {
    text.trim_matches(whitespace)
}

pub(super) fn lower(text: &str) -> String {
    // Go uses a simple one-rune mapping, rather than full string case folding.
    text.chars()
        .map(|c| c.to_lowercase().next().unwrap_or(c))
        .collect()
}

pub(super) fn decode_runes(mut bytes: &[u8]) -> String {
    let mut decoded = String::new();
    while !bytes.is_empty() {
        match std::str::from_utf8(bytes) {
            Ok(text) => {
                decoded.push_str(text);
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                if let Ok(prefix) = std::str::from_utf8(&bytes[..valid]) {
                    decoded.push_str(prefix);
                }
                // Go range consumes exactly one byte for every malformed rune.
                decoded.push('\u{fffd}');
                bytes = &bytes[valid + 1..];
            }
        }
    }
    decoded
}

pub(super) fn split_sentences(text: &str, newline: bool) -> Vec<&str> {
    text.split(|c| matches!(c, '.' | '?' | '!') || (newline && c == '\n'))
        .filter(|part| !trim(part).is_empty())
        .collect()
}
