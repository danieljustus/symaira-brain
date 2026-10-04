//! Go bare-value diagnostics for errors already rejected by the native parser.
use crate::{GoText, config::format_go_quoted_bytes};

// This is an error classifier, never a second accepting TOML parser. Limit it
// to complete ASCII bare keys and invalid bare words before the parser's first
// failure. Quoted/multiline keys and other syntax retain native diagnostics.
pub(super) fn bare_value_error(bytes: &[u8], limit: usize) -> Option<GoText> {
    let mut offset = 0;
    let mut table = Vec::new();
    for (index, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
        if offset >= limit {
            break;
        }
        let trimmed = trim(line);
        if trimmed.starts_with(b"[") {
            if let Some(inner) = trimmed
                .strip_prefix(b"[")
                .and_then(|v| v.strip_suffix(b"]"))
                && bare_key(inner)
            {
                table = inner.to_vec();
            }
            offset += line.len() + 1;
            continue;
        }
        let Some(equal) = line.iter().position(|byte| *byte == b'=') else {
            offset += line.len() + 1;
            continue;
        };
        let key = trim(&line[..equal]);
        if !bare_key(key) {
            offset += line.len() + 1;
            continue;
        }
        let mut cursor = equal + 1;
        let mut quote = None;
        while cursor < line.len() && offset + cursor < limit {
            let byte = line[cursor];
            if let Some(active) = quote {
                if byte == b'\\' && active == b'"' {
                    cursor += 2;
                    continue;
                }
                if byte == active {
                    quote = None;
                }
            } else if byte == b'#' {
                break;
            } else if matches!(byte, b'\'' | b'"') {
                // Multiline strings are outside this classifier's admission.
                if line
                    .get(cursor..cursor + 3)
                    .is_some_and(|v| v.iter().all(|b| *b == byte))
                {
                    return None;
                }
                quote = Some(byte);
            } else if byte == b'{' {
                return None;
            } else if byte.is_ascii_alphabetic() || byte == b'_' {
                let start = cursor;
                while cursor < line.len()
                    && !matches!(
                        line[cursor],
                        b' ' | b'\t' | b'\r' | b',' | b']' | b'}' | b'#'
                    )
                {
                    cursor += 1;
                }
                let word = &line[start..cursor];
                if !matches!(word, b"true" | b"false" | b"inf" | b"nan") {
                    let mut full_key = table.clone();
                    if !full_key.is_empty() {
                        full_key.push(b'.');
                    }
                    full_key.extend_from_slice(key);
                    return Some(
                        format!(
                            "toml: line {} (last key {}): expected value but found {} instead",
                            index + 1,
                            format_go_quoted_bytes(&full_key),
                            format_go_quoted_bytes(word),
                        )
                        .into(),
                    );
                }
                continue;
            } else if byte.is_ascii_digit() || matches!(byte, b'+' | b'-') {
                // Numeric/datetime tokens can contain alphabetic exponent or
                // timezone characters. Their complete grammar stays parser-owned.
                while cursor < line.len()
                    && !matches!(
                        line[cursor],
                        b' ' | b'\t' | b'\r' | b',' | b']' | b'}' | b'#'
                    )
                {
                    cursor += 1;
                }
                continue;
            }
            cursor += 1;
        }
        offset += line.len() + 1;
    }
    None
}
fn trim(bytes: &[u8]) -> &[u8] {
    let begin = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(begin, |i| i + 1);
    &bytes[begin..end]
}
fn bare_key(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

#[cfg(test)]
mod tests {
    #[test]
    fn rejected_bare_array_word_precedes_later_invalid_utf8() {
        let error = super::super::document::parse(b"invalid=[unterminated\n#\xff\n").unwrap_err();
        assert_eq!(error.as_ref(), b"toml: line 1 (last key \"invalid\"): expected value but found \"unterminated\" instead");
        let error = super::super::document::parse(b"#\xff\n").unwrap_err();
        assert_eq!(error.as_ref(), b"toml: line 1: invalid UTF-8 byte: 0xff");
    }

    #[test]
    fn classifier_never_accepts_repaired_bytes_or_scans_string_values() {
        assert!(
            super::super::document::parse(b"key='unterminated'\nnumber=1e3\nflag=true\n").is_ok()
        );
        assert!(super::super::document::parse(b"key='unterminated\xff'\n").is_err());
        assert!(
            super::bare_value_error(b"key='''line\ninvalid = word\n'''\n", usize::MAX).is_none()
        );
        assert!(super::bare_value_error(b"key={bad=word}\n", usize::MAX).is_none());
    }
}
