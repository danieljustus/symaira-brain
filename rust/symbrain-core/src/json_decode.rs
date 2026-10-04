//! Additive Go string-decoding seam; no accepting caller changes globally.
// Reuses the existing provider-config byte/surrogate primitive unchanged.
/// Repairs only JSON string UTF8/surrogate representation as Go does.
///
/// # Panics
/// Panics only if an internal UTF8 boundary invariant is violated.
#[must_use]
pub fn go_json_compatible_text(blob: &[u8]) -> String {
    // encoding/json decodes invalid UTF-8 with utf8.DecodeRune, replacing one
    // invalid byte at a time. Rust's from_utf8_lossy groups some invalid
    // prefixes, so preserve Go's bytewise replacement explicitly.
    let mut utf8 = String::with_capacity(blob.len());
    let mut remaining = blob;
    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                utf8.push_str(valid);
                break;
            }
            Err(error) => {
                let valid_end = error.valid_up_to();
                utf8.push_str(
                    std::str::from_utf8(&remaining[..valid_end])
                        .expect("valid_up_to marks a UTF-8 boundary"),
                );
                utf8.push('\u{fffd}');
                remaining = &remaining[valid_end + 1..];
            }
        }
    }

    // Go's JSON decoder replaces unpaired UTF-16 surrogate escapes with
    // U+FFFD; serde_json rejects them. Normalize only string escapes, leaving
    // malformed JSON escapes for serde_json to reject as before.
    let bytes = utf8.as_bytes();
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut index = 0;
    let mut in_string = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if !in_string {
            normalized.push(byte);
            index += 1;
            if byte == b'"' {
                in_string = true;
            }
            continue;
        }
        if byte == b'"' {
            normalized.push(byte);
            index += 1;
            in_string = false;
            continue;
        }
        if byte != b'\\' {
            normalized.push(byte);
            index += 1;
            continue;
        }

        if let Some(first) = unicode_escape_unit(bytes, index) {
            if (0xd800..=0xdbff).contains(&first) {
                if let Some(second) = unicode_escape_unit(bytes, index + 6)
                    && (0xdc00..=0xdfff).contains(&second)
                {
                    normalized.extend_from_slice(&bytes[index..index + 12]);
                    index += 12;
                    continue;
                }
                normalized.extend_from_slice(b"\\uFFFD");
                index += 6;
                continue;
            }
            if (0xdc00..=0xdfff).contains(&first) {
                normalized.extend_from_slice(b"\\uFFFD");
                index += 6;
                continue;
            }
            normalized.extend_from_slice(&bytes[index..index + 6]);
            index += 6;
            continue;
        }

        normalized.push(byte);
        index += 1;
        if index < bytes.len() {
            normalized.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(normalized).expect("normalized JSON text remains valid UTF-8")
}

fn unicode_escape_unit(bytes: &[u8], start: usize) -> Option<u16> {
    let escape = bytes.get(start..start.checked_add(6)?)?;
    if escape[0] != b'\\' || escape[1] != b'u' {
        return None;
    }
    let mut value = 0u16;
    for digit in &escape[2..] {
        value = value.checked_mul(16)? + u16::from(hex_digit(*digit)?);
    }
    Some(value)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn preserves_go_byte_and_surrogate_replacement_without_accepting_json() {
        let repaired = super::go_json_compatible_text(b"\"\xe2\x82\\ud800\\udc00\\ud800\"");
        let parsed: String = serde_json::from_str(&repaired).unwrap();
        assert_eq!(parsed, "\u{fffd}\u{fffd}\u{10000}\u{fffd}");
        assert!(
            serde_json::from_str::<String>(&super::go_json_compatible_text(b"\"\\q\"")).is_err()
        );
        assert_eq!(
            super::go_json_compatible_text(br#""\u1234\\ud800""#),
            r#""\u1234\\ud800""#
        );
    }
}
