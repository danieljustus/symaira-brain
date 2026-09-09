//! Lossless UTF-8 transcoding with a prefix-escaped reversible codec.

const SENTINEL: char = '\u{F000}';
const BYTE_TAG_BASE: u32 = 0xF100;
const BYTE_TAG_MAX: u32 = 0xF1FF;

fn push_valid_utf8(out: &mut String, valid: &str) {
    if valid.contains(SENTINEL) {
        for c in valid.chars() {
            if c == SENTINEL {
                out.push(SENTINEL);
                out.push(SENTINEL);
            } else {
                out.push(c);
            }
        }
    } else {
        out.push_str(valid);
    }
}

fn push_invalid_byte(out: &mut String, b: u8) {
    out.push(SENTINEL);
    let tag = char::from_u32(BYTE_TAG_BASE + u32::from(b)).expect("valid PUA codepoint");
    out.push(tag);
}

/// Encodes raw bytes into a valid UTF-8 string using a prefix-escaped reversible codec.
///
/// Valid Unicode codepoints are preserved, with occurrences of the sentinel codepoint
/// (`U+F000`) escaped as `[SENTINEL, SENTINEL]`. Invalid UTF-8 bytes are encoded as a
/// two-character sequence: `[SENTINEL, U+F100 + byte]`.
#[must_use]
pub fn to_lossless_str(raw: &[u8]) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        match std::str::from_utf8(&raw[i..]) {
            Ok(valid) => {
                push_valid_utf8(&mut out, valid);
                break;
            }
            Err(err) => {
                let valid_up_to = err.valid_up_to();
                if valid_up_to > 0 {
                    if let Ok(valid) = std::str::from_utf8(&raw[i..i + valid_up_to]) {
                        push_valid_utf8(&mut out, valid);
                    }
                    i += valid_up_to;
                }
                if let Some(error_len) = err.error_len() {
                    for &b in &raw[i..i + error_len] {
                        push_invalid_byte(&mut out, b);
                    }
                    i += error_len;
                } else {
                    for &b in &raw[i..] {
                        push_invalid_byte(&mut out, b);
                    }
                    break;
                }
            }
        }
    }
    out
}

/// Decodes a string produced by `to_lossless_str` back to the original raw bytes.
#[must_use]
pub fn from_lossless_str(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut chars = s.chars();
    let mut buf = [0u8; 4];
    while let Some(c) = chars.next() {
        if c == SENTINEL {
            match chars.next() {
                Some(SENTINEL) | None => {
                    out.extend_from_slice(SENTINEL.encode_utf8(&mut buf).as_bytes());
                }
                Some(next) if (BYTE_TAG_BASE..=BYTE_TAG_MAX).contains(&(next as u32)) => {
                    #[allow(clippy::cast_possible_truncation)]
                    out.push((next as u32 - BYTE_TAG_BASE) as u8);
                }
                Some(other) => {
                    out.extend_from_slice(SENTINEL.encode_utf8(&mut buf).as_bytes());
                    out.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
                }
            }
        } else {
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_valid_ascii_and_unicode() {
        let cases: &[&[u8]] = &[
            b"",
            b"hello world",
            "caf\u{00e9}".as_bytes(),
            "\u{1f600}\u{6771}\u{4eac}".as_bytes(),
            "\u{0085}\u{00a0}\u{200b}\u{2028}\u{feff}".as_bytes(),
        ];
        for &raw in cases {
            let encoded = to_lossless_str(raw);
            assert_eq!(from_lossless_str(&encoded), raw);
        }
    }

    #[test]
    fn roundtrip_arbitrary_invalid_bytes() {
        let cases: &[&[u8]] = &[
            b"\xff",
            b"\x80",
            b"\xfe",
            b"\xc0\xaf",
            b"\xed\xa0\x80",
            b"prefix\xffmiddle\x80suffix",
            b"\xff\xff\xff",
        ];
        for &raw in cases {
            let encoded = to_lossless_str(raw);
            assert_eq!(from_lossless_str(&encoded), raw);
        }
    }

    #[test]
    fn roundtrip_literal_pua_and_sentinels() {
        // U+F015 must not collide with byte 0x15
        let pua_f015 = "\u{F015}".as_bytes();
        let encoded_f015 = to_lossless_str(pua_f015);
        assert_eq!(from_lossless_str(&encoded_f015), pua_f015);

        // U+F000 sentinel must not collide with byte 0x00
        let pua_f000 = "\u{F000}".as_bytes();
        let encoded_f000 = to_lossless_str(pua_f000);
        assert_eq!(encoded_f000, "\u{F000}\u{F000}");
        assert_eq!(from_lossless_str(&encoded_f000), pua_f000);

        // U+F0FF must not collide with byte 0xFF
        let pua_f0ff = "\u{F0FF}".as_bytes();
        let encoded_f0ff = to_lossless_str(pua_f0ff);
        assert_eq!(from_lossless_str(&encoded_f0ff), pua_f0ff);
    }

    #[test]
    fn roundtrip_mixing_literal_pua_and_raw_bytes() {
        let mut mixed = Vec::new();
        mixed.extend_from_slice("\u{F015}".as_bytes());
        mixed.extend_from_slice("\u{F000}".as_bytes());
        mixed.push(0xff);
        mixed.push(0x15);
        mixed.extend_from_slice("\u{F015}".as_bytes());
        mixed.push(0xff);
        mixed.extend_from_slice("normal_text".as_bytes());

        let encoded = to_lossless_str(&mixed);
        assert_eq!(from_lossless_str(&encoded), mixed);
    }

    #[test]
    fn distinct_encoding_for_pua_and_raw_byte() {
        let pua_bytes = "\u{F015}".as_bytes();
        let raw_bytes = &[0xff, 0x15];

        let encoded_pua = to_lossless_str(pua_bytes);
        let encoded_raw = to_lossless_str(raw_bytes);

        assert_ne!(encoded_pua, encoded_raw);
        assert_eq!(from_lossless_str(&encoded_pua), pua_bytes);
        assert_eq!(from_lossless_str(&encoded_raw), raw_bytes);

        let literal_f0ff = "\u{F0FF}".as_bytes();
        let raw_ff = &[0xff];
        assert_ne!(to_lossless_str(literal_f0ff), to_lossless_str(raw_ff));
        assert_eq!(
            from_lossless_str(&to_lossless_str(literal_f0ff)),
            literal_f0ff
        );
        assert_eq!(from_lossless_str(&to_lossless_str(raw_ff)), raw_ff);
    }

    #[test]
    fn roundtrip_all_single_bytes() {
        for b in 0u8..=255 {
            let raw = [b];
            let encoded = to_lossless_str(&raw);
            assert_eq!(from_lossless_str(&encoded), raw);
        }
    }
}
