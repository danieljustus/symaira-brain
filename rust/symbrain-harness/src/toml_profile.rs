//! Codex profile strings retain BurntSushi1.6's byte-valued quoting.
//! Quoting rules derive from TOML authors (MIT), retained in
//! migration/fixtures/brain-config13/toml-authors-LICENSE.txt.
use crate::HarnessError;
use std::collections::BTreeMap;

pub(super) fn marshal(
    document: &toml_edit::DocumentMut,
    servers_key: &str,
    profiles: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>, HarnessError> {
    let text = document.to_string();
    if profiles.is_empty() {
        return Ok(text.into_bytes());
    }
    // Obtain the exact generated value spans; no placeholder, global string
    // replacement or reparsing of the final potentially non-UTF8 output.
    let parsed = toml_edit::Document::parse(text.as_str())?;
    let mut replacements = Vec::new();
    for (name, profile) in profiles {
        let span = parsed
            .get(servers_key)
            .and_then(|servers| servers.get(name))
            .and_then(|server| server.get("args"))
            .and_then(toml_edit::Item::as_array)
            .and_then(|args| args.get(2))
            .and_then(toml_edit::Value::span)
            .ok_or_else(|| HarnessError::Encode("missing generated profile string span".into()))?;
        replacements.push((span, quote(profile)));
    }
    replacements.sort_by_key(|(span, _)| span.start);
    let mut previous_end = 0;
    for (span, _) in &replacements {
        if span.start < previous_end || span.end > text.len() {
            return Err(HarnessError::Encode(
                "invalid generated profile string span".into(),
            ));
        }
        previous_end = span.end;
    }
    let mut bytes = text.into_bytes();
    for (span, quoted) in replacements.into_iter().rev() {
        bytes.splice(span, quoted);
    }
    Ok(bytes)
}

fn quote(raw: &[u8]) -> Vec<u8> {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut bytes = vec![b'"'];
    for byte in raw {
        match byte {
            b'"' => bytes.extend_from_slice(b"\\\""),
            b'\\' => bytes.extend_from_slice(b"\\\\"),
            8 => bytes.extend_from_slice(b"\\b"),
            9 => bytes.extend_from_slice(b"\\t"),
            10 => bytes.extend_from_slice(b"\\n"),
            12 => bytes.extend_from_slice(b"\\f"),
            13 => bytes.extend_from_slice(b"\\r"),
            0..=31 | 127 => bytes.extend_from_slice(&[
                b'\\',
                b'u',
                b'0',
                b'0',
                HEX[usize::from(byte >> 4)],
                HEX[usize::from(byte & 15)],
            ]),
            _ => bytes.push(*byte),
        }
    }
    bytes.push(b'"');
    bytes
}

#[cfg(test)]
mod tests {
    use super::quote;

    #[test]
    fn pinned_replacer_handles_controls_quotes_and_passes_non_ascii_bytes() {
        assert_eq!(
            quote(b"\x00\x07\x08\t\n\x0b\x0c\r\x0e\x1f\x7f\"\\"),
            br#""\u0000\u0007\b\t\n\u000b\f\r\u000e\u001f\u007f\"\\""#
        );
        for byte in 128..=255 {
            assert_eq!(quote(&[byte]), [b'"', byte, b'"']);
        }
        assert_eq!(
            quote("literal�\u{2028}<>&'".as_bytes()),
            "\"literal�\u{2028}<>&'\"".as_bytes()
        );
    }
}
