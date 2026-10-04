//! Iterative Go encoding/json syntax admission, before any typed Skills field.
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
fn error(byte: u8, context: &str) -> String {
    // Go scanner.quoteChar delegates a one-byte string to strconv.Quote.
    // It displays UTF-8 bytes as Latin-1 codepoints, rather than losing them.
    let quoted = if byte == b'\'' {
        "'\\''".into()
    } else if byte == b'"' {
        "'\"'".into()
    } else {
        let text = char::from(byte).to_string();
        let quoted = symbrain_core::config::format_go_quoted_bytes(text.as_bytes());
        format!("'{}'", &quoted[1..quoted.len() - 1])
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
                let escape = next(raw, *index)?;
                *index += 1;
                match escape {
                    b'b' | b'f' | b'n' | b'r' | b't' | b'\\' | b'/' | b'"' => {}
                    b'u' => {
                        for _ in 0..4 {
                            let hex = next(raw, *index)?;
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
        let byte = next(raw, *index)?;
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
    let byte = next(raw, *index)?;
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
    match next(raw, *index)? {
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
