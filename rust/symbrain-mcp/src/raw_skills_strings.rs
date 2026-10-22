//! Go per-byte/string repair reused from the retained Skills marker codec.
fn hex_quad(bytes: &[u8]) -> Option<u16> {
    let text = std::str::from_utf8(bytes).ok()?;
    u16::from_str_radix(text, 16).ok()
}

// encoding/json replaces each invalid UTF-8 byte and unpaired UTF-16 surrogate
// inside strings with U+FFFD. Preserve escapes and valid surrogate pairs so the
// normal JSON parser still rejects malformed syntax and field types.
pub(super) fn repair(raw: &[u8], surrogates: bool) -> String {
    let mut utf8 = String::new();
    let mut remaining = raw;
    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(text) => {
                utf8.push_str(text);
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                utf8.push_str(std::str::from_utf8(&remaining[..valid]).unwrap_or_default());
                utf8.push('\u{fffd}');
                remaining = &remaining[valid + 1..];
            }
        }
    }
    let bytes = utf8.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let (mut i, mut in_string) = (0, false);
    while i < bytes.len() {
        if bytes[i] == b'"' {
            in_string = !in_string;
        }
        if in_string && bytes[i] == b'\\' && i + 1 < bytes.len() {
            if surrogates
                && bytes[i + 1] == b'u'
                && i + 6 <= bytes.len()
                && let Some(code) = hex_quad(&bytes[i + 2..i + 6])
                && (0xd800..=0xdfff).contains(&code)
            {
                if code <= 0xdbff
                    && i + 12 <= bytes.len()
                    && &bytes[i + 6..i + 8] == b"\\u"
                    && hex_quad(&bytes[i + 8..i + 12])
                        .is_some_and(|v| (0xdc00..=0xdfff).contains(&v))
                {
                    result.extend_from_slice(&bytes[i..i + 12]);
                    i += 12;
                } else {
                    result.extend_from_slice(b"\\ufffd");
                    i += 6;
                }
                continue;
            }
            result.extend_from_slice(&bytes[i..i + 2]);
            i += 2;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(result).expect("only ASCII escapes were replaced in UTF-8")
}

// Repair only string tokens. Invalid bytes outside strings remain parse errors.
// Original argument surrogate escapes stay RawMessage until their own decoder;
// only envelope/params keys need immediate repair for ordered field admission.
pub(super) fn transport(raw: &[u8], key_depth: usize) -> Option<String> {
    let (mut out, mut i, mut depth) = (String::new(), 0, 0_usize);
    while i < raw.len() {
        if raw[i] != b'"' {
            let byte = raw[i];
            if !byte.is_ascii() {
                return None;
            }
            match byte {
                b'{' | b'[' => depth += 1,
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
            out.push(char::from(byte));
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        loop {
            match raw.get(i)? {
                b'\\' => i += 2,
                b'"' => {
                    i += 1;
                    break;
                }
                _ => i += 1,
            }
        }
        let key = raw[i..].iter().find(|b| !b.is_ascii_whitespace()) == Some(&b':');
        out.push_str(&repair(&raw[start..i], key && depth <= key_depth));
    }
    Some(out)
}

// Go strings.TrimSpace decodes each edge rune independently, even when the
// interior contains invalid bytes. Unknown RPC owners still perform their
// previous whole-line UTF-8 check before ordinary dispatch.
pub(super) fn trim_go_space(mut raw: &[u8]) -> &[u8] {
    loop {
        let head = &raw[..raw.len().min(4)];
        let valid =
            std::str::from_utf8(head).map_or_else(|error| &head[..error.valid_up_to()], |_| head);
        let character = std::str::from_utf8(valid)
            .ok()
            .and_then(|text| text.chars().next());
        let Some(character) = character.filter(|ch| go_space(*ch)) else {
            break;
        };
        raw = &raw[character.len_utf8()..];
    }
    loop {
        let mut trim = 0;
        for width in 1..=raw.len().min(4) {
            if let Ok(text) = std::str::from_utf8(&raw[raw.len() - width..]) {
                let mut chars = text.chars();
                if chars.next().is_some_and(go_space) && chars.next().is_none() {
                    trim = width;
                    break;
                }
            }
        }
        if trim == 0 {
            break;
        }
        raw = &raw[..raw.len() - trim];
    }
    raw
}
fn go_space(ch: char) -> bool {
    matches!(ch, '\u{0009}'..='\u{000d}' | ' ' | '\u{0085}' | '\u{00a0}' | '\u{1680}'
        | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}')
}
