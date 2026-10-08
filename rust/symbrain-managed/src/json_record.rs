//! Ordered, typed Go JSON records for managed version and provenance reads.
#[path = "json_syntax.rs"]
mod syntax;

pub(super) fn fields<'a>(
    bytes: &'a [u8],
    type_name: &str,
) -> Result<Vec<(String, &'a [u8])>, String> {
    syntax::validate(bytes)?;
    let bytes = trim(bytes);
    if bytes == b"null" {
        return Ok(Vec::new());
    }
    if bytes[0] != b'{' {
        return Err(format!(
            "json: cannot unmarshal {} into Go value of type {type_name}",
            kind(bytes)
        ));
    }
    let mut remaining = trim(&bytes[1..]);
    let mut fields = Vec::new();
    while remaining[0] != b'}' {
        let key_len = token_length(remaining);
        let key: String = serde_json::from_slice(
            &super::provenance::json::replace_invalid_strings(&remaining[..key_len]),
        )
        .map_err(|error| error.to_string())?;
        remaining = trim(&remaining[key_len..]);
        remaining = trim(&remaining[1..]); // validated colon
        let value_len = token_length(remaining);
        fields.push((key, &remaining[..value_len]));
        remaining = trim(&remaining[value_len..]);
        if remaining[0] == b',' {
            remaining = trim(&remaining[1..]);
        }
    }
    Ok(fields)
}

fn trim(mut bytes: &[u8]) -> &[u8] {
    while bytes
        .first()
        .is_some_and(|b| matches!(b, b' ' | b'\r' | b'\n' | b'\t'))
    {
        bytes = &bytes[1..];
    }
    while bytes
        .last()
        .is_some_and(|b| matches!(b, b' ' | b'\r' | b'\n' | b'\t'))
    {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

// Syntax is already checked. Scan raw ranges without recursion, numeric
// conversion, UTF-8 replacement, or string unescaping. time.Time needs its
// literal JSON token, while ordinary string fields are decoded separately.
fn token_length(bytes: &[u8]) -> usize {
    let mut depth = 0;
    let mut in_string = false;
    let mut escaped = false;
    for (index, &byte) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
                if depth == 0 {
                    return index + 1;
                }
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                if depth == 0 {
                    return index;
                }
                depth -= 1;
                if depth == 0 {
                    return index + 1;
                }
            }
            b',' | b' ' | b'\r' | b'\n' | b'\t' if depth == 0 => return index,
            _ => {}
        }
    }
    bytes.len()
}

pub(super) fn fold(key: &str) -> String {
    key.replace('\u{017f}', "s")
        .replace('\u{212a}', "k")
        .to_ascii_lowercase()
}

pub(super) fn string(token: &[u8], field: &str, type_name: &str) -> Result<Option<String>, String> {
    if token == b"null" {
        return Ok(None);
    }
    if !token.starts_with(b"\"") {
        return Err(format!(
            "json: cannot unmarshal {} into Go struct field {field} of type {type_name}",
            kind(token)
        ));
    }
    serde_json::from_slice(&super::provenance::json::replace_invalid_strings(token))
        .map(Some)
        .map_err(|error| error.to_string())
}

fn kind(token: &[u8]) -> &'static str {
    match token[0] {
        b'[' => "array",
        b'{' => "object",
        b'"' => "string",
        b't' | b'f' => "bool",
        _ => "number",
    }
}

pub(super) fn version(bytes: &[u8]) -> Result<String, String> {
    let mut version = String::new();
    let mut first_error = None;
    for (key, raw) in fields(bytes, r#"struct { Version string "json:\"version\"" }"#)? {
        if fold(&key) == "version" {
            match string(raw, ".version", "string") {
                Ok(Some(value)) => version = value,
                Ok(None) => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
    }
    first_error.map_or(Ok(version), Err)
}
