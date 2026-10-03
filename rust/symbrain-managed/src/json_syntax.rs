//! encoding/json syntax diagnostics for the frozen managed CLI contract.
//!
//! The iterative state machine avoids recursive descent on untrusted version
//! output and retains Go's 10,000-container limit and byte-level errors.
#[derive(Clone, Copy)]
enum State {
    Value,
    ArrayStart,
    ObjectStart,
    Key,
    String,
    Escape,
    Unicode(u8),
    Negative,
    Integer,
    Zero,
    Dot,
    Fraction,
    Exponent,
    ExponentSign,
    ExponentDigits,
    Literal(&'static [u8], usize),
    End,
    Top,
}
#[derive(Clone, Copy)]
enum Container {
    Key,
    Object,
    Array,
}

pub(super) fn validate(bytes: &[u8]) -> Result<(), String> {
    let mut state = State::Value;
    let mut stack = Vec::new();
    for &byte in bytes {
        step(&mut state, &mut stack, byte)?;
    }
    if matches!(state, State::Top) {
        return Ok(());
    }
    // Go eof feeds a space to finish scalar values. A malformed literal,
    // number or escape can fail on this synthetic byte; retain that error.
    step(&mut state, &mut stack, b' ')?;
    if matches!(state, State::Top) {
        Ok(())
    } else {
        Err("unexpected end of JSON input".into())
    }
}

fn space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\r' | b'\n' | b'\t')
}

fn invalid(byte: u8, context: &str) -> String {
    let quoted = match byte {
        b'\'' => "'\\\''".into(),
        b'"' => "'\"'".into(),
        _ => {
            let value = char::from(byte).to_string();
            let quoted = symbrain_core::config::format_go_quoted(value.as_ref());
            format!("'{}'", &quoted[1..quoted.len() - 1])
        }
    };
    format!("invalid character {quoted} {context}")
}

fn step(state: &mut State, stack: &mut Vec<Container>, byte: u8) -> Result<(), String> {
    loop {
        *state = match *state {
            State::Value => match byte {
                byte if space(byte) => State::Value,
                b'{' | b'[' => {
                    if stack.len() == 10_000 {
                        return Err(invalid(byte, "exceeded max depth"));
                    }
                    stack.push(if byte == b'{' {
                        Container::Key
                    } else {
                        Container::Array
                    });
                    if byte == b'{' {
                        State::ObjectStart
                    } else {
                        State::ArrayStart
                    }
                }
                b'"' => State::String,
                b'-' => State::Negative,
                b'0' => State::Zero,
                b'1'..=b'9' => State::Integer,
                b't' => State::Literal(b"true", 1),
                b'f' => State::Literal(b"false", 1),
                b'n' => State::Literal(b"null", 1),
                _ => return Err(invalid(byte, "looking for beginning of value")),
            },
            State::ArrayStart => {
                if space(byte) {
                    return Ok(());
                }
                *state = if byte == b']' {
                    State::End
                } else {
                    State::Value
                };
                continue;
            }
            State::ObjectStart => {
                if space(byte) {
                    return Ok(());
                }
                if byte == b'}' {
                    *stack.last_mut().expect("open object") = Container::Object;
                    *state = State::End;
                } else {
                    *state = State::Key;
                }
                continue;
            }
            State::Key => match byte {
                byte if space(byte) => State::Key,
                b'"' => State::String,
                _ => return Err(invalid(byte, "looking for beginning of object key string")),
            },
            State::String | State::Escape | State::Unicode(_) => string_state(*state, byte)?,
            State::Negative
            | State::Integer
            | State::Zero
            | State::Dot
            | State::Fraction
            | State::Exponent
            | State::ExponentSign
            | State::ExponentDigits => {
                if let Some(next) = number_state(*state, byte)? {
                    next
                } else {
                    *state = State::End;
                    continue;
                }
            }
            State::Literal(literal, index) => {
                if byte != literal[index] {
                    return Err(invalid(
                        byte,
                        &format!(
                            "in literal {} (expecting '{}')",
                            std::str::from_utf8(literal).expect("ASCII literal"),
                            char::from(literal[index])
                        ),
                    ));
                }
                if index + 1 == literal.len() {
                    State::End
                } else {
                    State::Literal(literal, index + 1)
                }
            }
            State::End => end_state(stack, byte)?,
            State::Top => {
                if !space(byte) {
                    return Err(invalid(byte, "after top-level value"));
                }
                State::Top
            }
        };
        return Ok(());
    }
}

fn string_state(state: State, byte: u8) -> Result<State, String> {
    Ok(match state {
        State::String => match byte {
            b'"' => State::End,
            b'\\' => State::Escape,
            0..=0x1f => return Err(invalid(byte, "in string literal")),
            _ => State::String,
        },
        State::Escape => match byte {
            b'b' | b'f' | b'n' | b'r' | b't' | b'\\' | b'/' | b'"' => State::String,
            b'u' => State::Unicode(4),
            _ => return Err(invalid(byte, "in string escape code")),
        },
        State::Unicode(remaining) => {
            if !byte.is_ascii_hexdigit() {
                return Err(invalid(byte, "in \\u hexadecimal character escape"));
            }
            if remaining == 1 {
                State::String
            } else {
                State::Unicode(remaining - 1)
            }
        }
        _ => unreachable!("string state"),
    })
}

fn end_state(stack: &mut Vec<Container>, byte: u8) -> Result<State, String> {
    if stack.is_empty() {
        return top_state(byte);
    }
    if space(byte) {
        return Ok(State::End);
    }
    Ok(match stack.last().expect("container") {
        Container::Key => {
            if byte != b':' {
                return Err(invalid(byte, "after object key"));
            }
            *stack.last_mut().expect("container") = Container::Object;
            State::Value
        }
        Container::Object => match byte {
            b',' => {
                *stack.last_mut().expect("container") = Container::Key;
                State::Key
            }
            b'}' => {
                stack.pop();
                State::End
            }
            _ => return Err(invalid(byte, "after object key:value pair")),
        },
        Container::Array => match byte {
            b',' => State::Value,
            b']' => {
                stack.pop();
                State::End
            }
            _ => return Err(invalid(byte, "after array element")),
        },
    })
}
fn top_state(byte: u8) -> Result<State, String> {
    if !space(byte) {
        return Err(invalid(byte, "after top-level value"));
    }
    Ok(State::Top)
}

fn number_state(state: State, byte: u8) -> Result<Option<State>, String> {
    Ok(Some(match state {
        State::Negative => match byte {
            b'0' => State::Zero,
            b'1'..=b'9' => State::Integer,
            _ => return Err(invalid(byte, "in numeric literal")),
        },
        State::Integer if byte.is_ascii_digit() => State::Integer,
        State::Integer | State::Zero => match byte {
            b'.' => State::Dot,
            b'e' | b'E' => State::Exponent,
            _ => {
                return Ok(None);
            }
        },
        State::Dot => {
            if !byte.is_ascii_digit() {
                return Err(invalid(byte, "after decimal point in numeric literal"));
            }
            State::Fraction
        }
        State::Fraction => match byte {
            byte if byte.is_ascii_digit() => State::Fraction,
            b'e' | b'E' => State::Exponent,
            _ => {
                return Ok(None);
            }
        },
        State::Exponent => {
            if matches!(byte, b'+' | b'-') {
                State::ExponentSign
            } else {
                return number_state(State::ExponentSign, byte);
            }
        }
        State::ExponentSign => {
            if !byte.is_ascii_digit() {
                return Err(invalid(byte, "in exponent of numeric literal"));
            }
            State::ExponentDigits
        }
        State::ExponentDigits => {
            if byte.is_ascii_digit() {
                State::ExponentDigits
            } else {
                return Ok(None);
            }
        }
        _ => unreachable!("number state"),
    }))
}

#[cfg(test)]
mod tests {
    use super::validate;
    #[test]
    fn truncated_scalar_errors_retain_go_eof_transition() {
        for (bytes, diagnostic) in [
            (b"-".as_slice(), "invalid character ' ' in numeric literal"),
            (
                b"1.".as_slice(),
                "invalid character ' ' after decimal point in numeric literal",
            ),
            (
                b"1e+".as_slice(),
                "invalid character ' ' in exponent of numeric literal",
            ),
            (
                b"\"\\".as_slice(),
                "invalid character ' ' in string escape code",
            ),
            (
                b"\"\\u0".as_slice(),
                "invalid character ' ' in \\u hexadecimal character escape",
            ),
            (
                b"nu".as_slice(),
                "invalid character ' ' in literal null (expecting 'l')",
            ),
            (b"\"abc".as_slice(), "unexpected end of JSON input"),
            (b"{".as_slice(), "unexpected end of JSON input"),
        ] {
            assert_eq!(validate(bytes).unwrap_err(), diagnostic);
        }
    }
}
