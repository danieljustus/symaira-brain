//! Go JSON byte repair, locally retained from the existing guard codec.
pub(super) fn quote_go_char(byte: u8) -> String {
    let escaped = match byte {
        b'\'' => "\\'".to_owned(),
        b'"' => "\"".to_owned(),
        b'\\' => "\\\\".to_owned(),
        0x07 => "\\a".to_owned(),
        0x08 => "\\b".to_owned(),
        0x0c => "\\f".to_owned(),
        b'\n' => "\\n".to_owned(),
        b'\r' => "\\r".to_owned(),
        b'\t' => "\\t".to_owned(),
        0x0b => "\\v".to_owned(),
        // Go's quoteChar calls strconv.Quote(string(c)); IsPrint excludes
        // Latin-1 spacing U+00A0 and soft hyphen U+00AD.
        0x20..=0x7e | 0xa1..=0xac | 0xae..=0xff => char::from(byte).to_string(),
        0x80..=0xa0 | 0xad => format!("\\u{byte:04x}"),
        _ => format!("\\x{byte:02x}"),
    };
    format!("'{escaped}'")
}

fn hex_quad(bytes: &[u8]) -> Option<u16> {
    let text = std::str::from_utf8(bytes).ok()?;
    u16::from_str_radix(text, 16).ok()
}

// encoding/json replaces each invalid UTF-8 byte and unpaired UTF-16 surrogate
// inside strings with U+FFFD. Preserve escapes and valid surrogate pairs so the
// normal JSON parser still rejects malformed syntax and field types.
pub(crate) fn repair_json_strings(raw: &[u8]) -> String {
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
            if bytes[i + 1] == b'u'
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
