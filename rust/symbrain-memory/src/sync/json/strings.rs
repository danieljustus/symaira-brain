//! Go unquoteBytes: malformed UTF8/surrogates become individual RuneErrors.
// Source-derived compatibility logic: Go1.26.7 SDK, Go Authors, BSD-3-Clause.
// Retained license: scripts/memory-sync-oracle/reference/GO-SDK-LICENSE.

use super::super::SyncError;
use super::raw::bad;

pub(super) fn string_end(data: &[u8], start: usize) -> Result<usize, SyncError> {
    let mut at = start + 1;
    while let Some(byte) = data.get(at).copied() {
        match byte {
            b'"' => return Ok(at + 1),
            b'\\' => {
                at += 1;
                let byte = *data
                    .get(at)
                    .ok_or_else(|| SyncError("unexpected EOF".into()))?;
                match byte {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                    b'u' => {
                        for index in 1..=4 {
                            let byte = *data
                                .get(at + index)
                                .ok_or_else(|| SyncError("unexpected EOF".into()))?;
                            if !byte.is_ascii_hexdigit() {
                                return Err(bad(byte, "in \\u hexadecimal character escape"));
                            }
                        }
                        at += 4;
                    }
                    _ => return Err(bad(byte, "in string escape code")),
                }
            }
            0..=31 => return Err(bad(byte, "in string literal")),
            _ => {}
        }
        at += 1;
    }
    Err(SyncError("unexpected EOF".into()))
}
fn hex(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0, |n, b| {
        n * 16
            + u16::from(match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                _ => b - b'A' + 10,
            })
    })
}
fn utf8(out: &mut String, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        match std::str::from_utf8(bytes) {
            Ok(text) => {
                out.push_str(text);
                return;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                out.push_str(std::str::from_utf8(&bytes[..valid]).expect("validated prefix"));
                out.push('\u{fffd}');
                bytes = &bytes[valid + 1..];
            }
        }
    }
}
pub(super) fn unquote(raw: &[u8]) -> Result<String, SyncError> {
    if raw.first() != Some(&b'"') {
        return Err(SyncError("expected JSON string".into()));
    }
    let mut out = String::new();
    let mut at = 1;
    let end = raw.len() - 1;
    while at < end {
        let start = at;
        while at < end && raw[at] != b'\\' {
            at += 1;
        }
        utf8(&mut out, &raw[start..at]);
        if at == end {
            break;
        }
        at += 1;
        match raw[at] {
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'/' => out.push('/'),
            b'b' => out.push('\u{8}'),
            b'f' => out.push('\u{c}'),
            b'n' => out.push('\n'),
            b'r' => out.push('\r'),
            b't' => out.push('\t'),
            b'u' => {
                let first = hex(&raw[at + 1..at + 5]);
                at += 4;
                let mut rune = u32::from(first);
                if (0xd800..=0xdbff).contains(&first)
                    && raw.get(at + 1..at + 3) == Some(b"\\u")
                    && at + 7 < raw.len()
                {
                    let second = hex(&raw[at + 3..at + 7]);
                    if (0xdc00..=0xdfff).contains(&second) {
                        rune = 0x10000
                            + ((u32::from(first) - 0xd800) << 10)
                            + (u32::from(second) - 0xdc00);
                        at += 6;
                    }
                }
                out.push(char::from_u32(rune).unwrap_or('\u{fffd}'));
            }
            _ => unreachable!("grammar validated string"),
        }
        at += 1;
    }
    Ok(out)
}

/// Protocol field names are ASCII. Only Kelvin K and long-s fold into ASCII
/// from outside ASCII under Go unicode.SimpleFold; no full uppercase expansion.
pub(super) fn field(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '\u{212a}' => 'k',
            '\u{17f}' => 's',
            _ => c.to_ascii_lowercase(),
        })
        .collect()
}
