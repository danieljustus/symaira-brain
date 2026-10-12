//! Conflict-row JSON errors, using the existing raw_skills_syntax scanner.
//! Crate-local copy avoids a new memory -> MCP/core dependency or shared API
//! change. The source scanner remains untouched and is bound in the handoff.
//! Accepts invalid UTF-8 only inside strings (the separate string codec repairs
//! it), preserves original number spans, and counts every container, including
//! ignored fields. No Value tree or recursive subtree copies are constructed.
//! Go scanner.go grammar/error contexts: Copyright Go Authors, BSD license
//! retained in migration/licenses/go-strconv-bsd.txt.
const MAX_DEPTH: usize = 10_000;

#[derive(Clone, Copy)]
enum State {
    ObjectFirst,
    ObjectKey,
    ObjectColon,
    ObjectValue,
    ObjectEnd,
    ArrayFirst,
    ArrayValue,
    ArrayEnd,
}

pub(super) fn validate(raw: &[u8]) -> Result<(), String> {
    scan(raw, |_| {})
}

// Number callbacks occur in original encounter order, including duplicates.
pub(super) fn scan(raw: &[u8], mut number: impl FnMut(&[u8])) -> Result<(), String> {
    let mut index = 0;
    let mut stack = Vec::new();
    value(raw, &mut index, &mut stack, &mut number)?;
    while let Some(state) = stack.last().copied() {
        space(raw, &mut index);
        let byte = next(raw, index)?;
        match state {
            State::ObjectFirst if byte == b'}' => {
                stack.pop();
                index += 1;
            }
            State::ArrayFirst if byte == b']' => {
                stack.pop();
                index += 1;
            }
            State::ObjectFirst | State::ObjectKey => {
                if byte != b'"' {
                    return Err(error(byte, "looking for beginning of object key string"));
                }
                string(raw, &mut index)?;
                *stack.last_mut().expect("object state") = State::ObjectColon;
            }
            State::ObjectColon => {
                if byte != b':' {
                    return Err(error(byte, "after object key"));
                }
                index += 1;
                *stack.last_mut().expect("object state") = State::ObjectValue;
            }
            State::ObjectValue | State::ArrayFirst | State::ArrayValue => {
                *stack.last_mut().expect("value state") = if matches!(state, State::ObjectValue) {
                    State::ObjectEnd
                } else {
                    State::ArrayEnd
                };
                value(raw, &mut index, &mut stack, &mut number)?;
            }
            State::ObjectEnd | State::ArrayEnd => {
                let object = matches!(state, State::ObjectEnd);
                if byte == if object { b'}' } else { b']' } {
                    stack.pop();
                    index += 1;
                } else if byte == b',' {
                    index += 1;
                    *stack.last_mut().expect("end state") = if object {
                        State::ObjectKey
                    } else {
                        State::ArrayValue
                    };
                } else {
                    return Err(error(
                        byte,
                        if object {
                            "after object key:value pair"
                        } else {
                            "after array element"
                        },
                    ));
                }
            }
        }
    }
    space(raw, &mut index);
    if let Some(&byte) = raw.get(index) {
        return Err(error(byte, "after top-level value"));
    }
    Ok(())
}

fn value(
    raw: &[u8],
    index: &mut usize,
    stack: &mut Vec<State>,
    number: &mut impl FnMut(&[u8]),
) -> Result<(), String> {
    space(raw, index);
    let byte = next(raw, *index)?;
    match byte {
        b'{' | b'[' => {
            if stack.len() == MAX_DEPTH {
                return Err(error(byte, "exceeded max depth"));
            }
            stack.push(if byte == b'{' {
                State::ObjectFirst
            } else {
                State::ArrayFirst
            });
            *index += 1;
        }
        b'"' => string(raw, index)?,
        b't' => literal(raw, index, b"true")?,
        b'f' => literal(raw, index, b"false")?,
        b'n' => literal(raw, index, b"null")?,
        b'-' | b'0'..=b'9' => {
            let start = *index;
            numeric(raw, index)?;
            number(&raw[start..*index]);
        }
        _ => return Err(error(byte, "looking for beginning of value")),
    }
    Ok(())
}

fn space(raw: &[u8], index: &mut usize) {
    while matches!(raw.get(*index), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        *index += 1;
    }
}
fn next(raw: &[u8], index: usize) -> Result<u8, String> {
    raw.get(index)
        .copied()
        .ok_or_else(|| "unexpected end of JSON input".into())
}
fn token_byte(raw: &[u8], index: usize) -> u8 {
    // Go scanner.eof feeds ASCII space to the active token state before using
    // the generic end error. Every required token byte below rejects that
    // space in its existing grammar/context; no synthetic input is admitted.
    // Containers and ordinary strings accept the space but remain incomplete,
    // so their lookups deliberately retain next's generic end error.
    raw.get(index).copied().unwrap_or(b' ')
}
fn error(byte: u8, context: &str) -> String {
    // Go scanner.quoteChar delegates a one-byte string to strconv.Quote.
    // It displays UTF-8 bytes as Latin-1 codepoints, rather than losing them.
    let quoted = if byte == b'\'' {
        "'\\''".into()
    } else if byte == b'"' {
        "'\"'".into()
    } else {
        // scanner.quoteChar quotes a single byte as a Latin-1 codepoint.
        let text = match byte {
            7 => "\\a".into(), 8 => "\\b".into(), 9 => "\\t".into(),
            10 => "\\n".into(), 11 => "\\v".into(), 12 => "\\f".into(), 13 => "\\r".into(),
            b'\\' => "\\\\".into(),
            0..=31 | 127 => format!("\\x{byte:02x}"),
            128..=160 | 173 => format!("\\u{byte:04x}"),
            _ => char::from(byte).to_string(),
        };
        format!("'{text}'")
    };
    format!("invalid character {quoted} {context}")
}

fn string(raw: &[u8], index: &mut usize) -> Result<(), String> {
    *index += 1;
    loop {
        let byte = next(raw, *index)?;
        *index += 1;
        match byte {
            b'"' => return Ok(()),
            b'\\' => {
                let escape = token_byte(raw, *index);
                *index += 1;
                match escape {
                    b'b' | b'f' | b'n' | b'r' | b't' | b'\\' | b'/' | b'"' => {}
                    b'u' => {
                        for _ in 0..4 {
                            let hex = token_byte(raw, *index);
                            if !hex.is_ascii_hexdigit() {
                                return Err(error(hex, "in \\u hexadecimal character escape"));
                            }
                            *index += 1;
                        }
                    }
                    _ => return Err(error(escape, "in string escape code")),
                }
            }
            0..=0x1f => return Err(error(byte, "in string literal")),
            _ => {}
        }
    }
}

fn literal(raw: &[u8], index: &mut usize, word: &[u8]) -> Result<(), String> {
    *index += 1;
    for &expected in &word[1..] {
        let byte = token_byte(raw, *index);
        if byte != expected {
            let word = std::str::from_utf8(word).expect("ASCII literal");
            return Err(error(
                byte,
                &format!("in literal {word} (expecting '{}')", char::from(expected)),
            ));
        }
        *index += 1;
    }
    Ok(())
}

fn digit(raw: &[u8], index: &mut usize, context: &str) -> Result<(), String> {
    let byte = token_byte(raw, *index);
    if !byte.is_ascii_digit() {
        return Err(error(byte, context));
    }
    while raw.get(*index).is_some_and(u8::is_ascii_digit) {
        *index += 1;
    }
    Ok(())
}
fn numeric(raw: &[u8], index: &mut usize) -> Result<(), String> {
    if raw.get(*index) == Some(&b'-') {
        *index += 1;
    }
    match token_byte(raw, *index) {
        b'0' => *index += 1,
        b'1'..=b'9' => digit(raw, index, "in numeric literal")?,
        byte => return Err(error(byte, "in numeric literal")),
    }
    if raw.get(*index) == Some(&b'.') {
        *index += 1;
        digit(raw, index, "after decimal point in numeric literal")?;
    }
    if matches!(raw.get(*index), Some(b'e' | b'E')) {
        *index += 1;
        if matches!(raw.get(*index), Some(b'+' | b'-')) {
            *index += 1;
        }
        digit(raw, index, "in exponent of numeric literal")?;
    }
    Ok(())
}

pub(super) fn metadata(raw: &str) -> Result<std::collections::BTreeMap<String, Option<String>>, String> {
    validate(raw.as_bytes())?;
    serde_json::from_str::<Option<std::collections::BTreeMap<String, Option<String>>>>(raw)
        .map(Option::unwrap_or_default)
        .map_err(|error| typed_error(&error, "map[string]string", "string"))
}

pub(super) fn embedding(raw: &str) -> Result<Vec<f32>, String> {
    validate(raw.as_bytes())?;
    let values = serde_json::from_str::<Option<Vec<Option<f32>>>>(raw)
        .map_err(|error| typed_error(&error, "[]float32", "float32"))?
        .unwrap_or_default().into_iter().map(Option::unwrap_or_default).collect::<Vec<_>>();
    if values.iter().any(|value| !value.is_finite()) {
        let mut overflow = None;
        scan(raw.as_bytes(), |number| {
            let text = std::str::from_utf8(number).expect("scanner admitted ASCII number");
            if overflow.is_none() && text.parse::<f32>().is_ok_and(|value| !value.is_finite()) {
                overflow = Some(format!("json: cannot unmarshal number {text} into Go value of type float32"));
            }
        })?;
        return Err(overflow.unwrap_or_else(|| "non-finite conflict embedding".into()));
    }
    Ok(values)
}

fn typed_error(error: &serde_json::Error, container: &str, element: &str) -> String {
    let message = error.to_string();
    let target = if message.contains("expected a map") || message.contains("expected a sequence") {
        container
    } else { element };
    let kind = if message.starts_with("invalid type: integer") || message.starts_with("invalid type: floating point") {
        "number"
    } else if message.starts_with("invalid type: boolean") { "bool"
    } else if message.starts_with("invalid type: string") { "string"
    } else if message.starts_with("invalid type: sequence") { "array"
    } else if message.starts_with("invalid type: map") { "object"
    } else {
        return message;
    };
    format!("json: cannot unmarshal {kind} into Go value of type {target}")
}

pub(super) fn row_error(index: usize, message: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, message)))
}
