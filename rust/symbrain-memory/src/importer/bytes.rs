use super::Metadata;

// DecodeRune consumes one byte on invalid UTF-8. Lossy conversion groups
// adjacent invalid bytes and would change Go JSON replacement counts.
pub(super) fn rune(bytes: &[u8]) -> (char, usize) {
    for length in 1..=bytes.len().min(4) {
        if let Ok(text) = std::str::from_utf8(&bytes[..length]) {
            if let Some(ch) = text.chars().next() {
                return (ch, length);
            }
        }
    }
    ('\u{fffd}', 1)
}

pub(super) fn space(ch: char) -> bool {
    matches!(ch, '\t'..='\r' | ' ' | '\u{0085}' | '\u{00a0}' | '\u{1680}'
        | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}'
        | '\u{205f}' | '\u{3000}')
}

pub(super) fn trim(bytes: &[u8]) -> &[u8] {
    let mut start = 0;
    let mut last = 0;
    let mut seen = false;
    let mut index = 0;
    while index < bytes.len() {
        let (ch, width) = rune(&bytes[index..]);
        if !space(ch) {
            if !seen {
                start = index;
                seen = true;
            }
            last = index + width;
        }
        index += width;
    }
    &bytes[start..last]
}

pub(super) fn fields(bytes: &[u8]) -> Vec<&[u8]> {
    let mut result = Vec::new();
    let mut start = None;
    let mut index = 0;
    while index < bytes.len() {
        let (ch, width) = rune(&bytes[index..]);
        if space(ch) {
            if let Some(begin) = start.take() {
                result.push(&bytes[begin..index]);
            }
        } else {
            start.get_or_insert(index);
        }
        index += width;
    }
    if let Some(begin) = start {
        result.push(&bytes[begin..]);
    }
    result
}

pub(super) fn quotes(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(|b| matches!(b, b'\'' | b'"')) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(|b| matches!(b, b'\'' | b'"')) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

pub(super) fn equal_fold(bytes: &[u8], ascii: &[u8]) -> bool {
    let mut index = 0;
    for expected in ascii {
        if index == bytes.len() {
            return false;
        }
        let (ch, width) = rune(&bytes[index..]);
        let lower = match ch {
            '\u{017f}' => 's',
            '\u{212a}' => 'k',
            _ => ch.to_ascii_lowercase(),
        };
        if u32::from(lower) != u32::from(expected.to_ascii_lowercase()) {
            return false;
        }
        index += width;
    }
    index == bytes.len()
}

pub(super) fn json_quote(bytes: &[u8]) -> Vec<u8> {
    let mut text = String::new();
    let mut index = 0;
    while index < bytes.len() {
        let (ch, width) = rune(&bytes[index..]);
        text.push(ch);
        index += width;
    }
    crate::gojson::quote(&text)
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
        .into_bytes()
}

pub(super) fn json_map(values: &Metadata) -> Vec<u8> {
    let members: Vec<Vec<u8>> = values
        .iter()
        .map(|(key, value)| {
            let mut member = json_quote(key);
            member.push(b':');
            member.extend(json_quote(value));
            member
        })
        .collect();
    enclosed(&members, b'{', b'}')
}

pub(super) fn json_list(values: &[Vec<u8>]) -> Vec<u8> {
    enclosed(
        &values.iter().map(|v| json_quote(v)).collect::<Vec<_>>(),
        b'[',
        b']',
    )
}

fn enclosed(values: &[Vec<u8>], begin: u8, end: u8) -> Vec<u8> {
    let mut out = vec![begin];
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(b',');
        }
        out.extend(value);
    }
    out.push(end);
    out
}

pub(super) fn value<'a>(metadata: &'a Metadata, key: &[u8]) -> &'a [u8] {
    metadata.get(key).map_or(&[], Vec::as_slice)
}

pub(super) fn set(metadata: &mut Metadata, key: &[u8], value: impl Into<Vec<u8>>) {
    metadata.insert(key.to_vec(), value.into());
}
