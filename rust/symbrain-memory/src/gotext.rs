//! Go string semantics over raw bytes.
//!
//! The shipped implementation keeps whatever bytes the caller passed: a Go
//! `string` is a byte sequence, `regexp` and `range` see every invalid byte as
//! one `U+FFFD`, and SQLite stores the raw bytes. Rust text types cannot hold
//! such input, so a [`GoView`] maps each invalid byte to a private-use
//! character from a 256-code-point block that does not occur in the text. A
//! regex or replacement running on the view therefore treats the byte like any
//! non-ASCII, non-space rune (exactly what `U+FFFD` is to Go), and
//! [`GoView::restore`] writes the original bytes back.

/// A byte string viewed as Rust text without losing invalid bytes.
#[derive(Debug, Clone)]
pub(crate) struct GoView {
    /// The text; invalid bytes appear as placeholder characters.
    pub text: String,
    /// First code point of the placeholder block (`None` for valid UTF-8).
    base: Option<u32>,
}

impl GoView {
    /// Views `bytes`; valid UTF-8 is returned unchanged.
    pub(crate) fn new(bytes: &[u8]) -> Self {
        if let Ok(text) = std::str::from_utf8(bytes) {
            return Self {
                text: text.to_owned(),
                base: None,
            };
        }
        let mut used = [false; 256];
        for chunk in bytes.utf8_chunks() {
            for character in chunk.valid().chars() {
                let code = u32::from(character);
                if code >= 0x10_0000 {
                    used[usize::try_from((code - 0x10_0000) >> 8).unwrap_or(0)] = true;
                }
            }
        }
        // Every block can only be taken by 256 distinct characters; with all
        // blocks in use an arbitrary one is reused (unreachable in practice).
        let block = (0..256).rev().find(|block| !used[*block]).unwrap_or(255);
        let base = 0x10_0000 + u32::try_from(block).unwrap_or(255) * 0x100;
        let mut text = String::with_capacity(bytes.len());
        for chunk in bytes.utf8_chunks() {
            text.push_str(chunk.valid());
            for byte in chunk.invalid() {
                text.push(char::from_u32(base + u32::from(*byte)).unwrap_or('\u{fffd}'));
            }
        }
        Self {
            text,
            base: Some(base),
        }
    }

    fn placeholder(&self, character: char) -> Option<u8> {
        let base = self.base?;
        let code = u32::from(character);
        (code >> 8 == base >> 8).then(|| u8::try_from(code & 0xff).unwrap_or(0))
    }

    /// Writes the original bytes back for `text` (a view-derived string).
    pub(crate) fn restore(&self, text: &str) -> Vec<u8> {
        if self.base.is_none() {
            return text.as_bytes().to_vec();
        }
        let mut out = Vec::with_capacity(text.len());
        let mut buffer = [0_u8; 4];
        for character in text.chars() {
            match self.placeholder(character) {
                Some(byte) => out.push(byte),
                None => out.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes()),
            }
        }
        out
    }

    /// The text as Go's rune iteration sees it: each invalid byte is U+FFFD.
    pub(crate) fn runes(&self) -> String {
        if self.base.is_none() {
            return self.text.clone();
        }
        self.text
            .chars()
            .map(|c| {
                if self.placeholder(c).is_some() {
                    '\u{fffd}'
                } else {
                    c
                }
            })
            .collect()
    }

    /// Converts a byte offset in the view to the offset in the original bytes.
    pub(crate) fn original_offset(&self, view_offset: usize) -> usize {
        if self.base.is_none() {
            return view_offset;
        }
        let mut original = 0;
        for (index, character) in self.text.char_indices() {
            if index >= view_offset {
                break;
            }
            original += if self.placeholder(character).is_some() {
                1
            } else {
                character.len_utf8()
            };
        }
        original
    }

    /// Whether the original bytes were not valid UTF-8.
    pub(crate) fn has_invalid(&self) -> bool {
        self.base.is_some()
    }
}

/// `encoding/json` string encoding of raw bytes under Go 1.27 (the v2 engine
/// with v1 semantics): an invalid byte is written as a literal U+FFFD.
pub(crate) fn json_string(bytes: &[u8]) -> String {
    let view = GoView::new(bytes);
    let mut out = String::with_capacity(view.text.len() + 2);
    out.push('"');
    for character in view.text.chars() {
        if view.placeholder(character).is_some() {
            out.push('\u{fffd}');
            continue;
        }
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '&' => out.push_str("\\u0026"),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if u32::from(c) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_text_round_trips_unchanged() {
        let view = GoView::new("héllo".as_bytes());
        assert!(!view.has_invalid());
        assert_eq!(view.restore(&view.text), "héllo".as_bytes());
    }

    #[test]
    fn invalid_bytes_survive_a_round_trip_one_rune_each() {
        let bytes = b"a\xf0\x9f\x98b\xffc";
        let view = GoView::new(bytes);
        assert!(view.has_invalid());
        assert_eq!(view.restore(&view.text), bytes);
        // Four invalid bytes -> four U+FFFD, like Go's range loop.
        assert_eq!(view.runes().matches('\u{fffd}').count(), 4);
    }

    #[test]
    fn placeholders_avoid_blocks_present_in_the_text() {
        let mut bytes = "\u{10ff80}".as_bytes().to_vec();
        bytes.push(0xff);
        let view = GoView::new(&bytes);
        assert_eq!(view.restore(&view.text), bytes);
    }

    #[test]
    fn json_strings_follow_encoding_json() {
        assert_eq!(
            json_string(b"a&b<c>\"\\\n\xffz\x01\x7f"),
            "\"a\\u0026b\\u003cc\\u003e\\\"\\\\\\n\u{fffd}z\\u0001\x7f\""
        );
    }
}
