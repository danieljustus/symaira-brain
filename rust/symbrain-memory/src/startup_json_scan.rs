//! Private rotation JSON grammar and original byte spans, before typed decode.
//! Go-derived rules: BSD-3-Clause license in migration/fixtures/brain-config13.
use std::ops::Range;
use symbrain_core::{GoText, config::format_go_quoted_bytes};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Array,
    Object,
    String,
    Number,
    Bool,
    Null,
}
pub(super) struct Node {
    pub kind: Kind,
    pub span: Range<usize>,
    pub parent: Option<usize>,
    pub key: Option<Range<usize>>,
}
#[derive(Clone, Copy)]
enum Expect {
    ArrayFirst,
    ArrayValue,
    ArrayAfter,
    ObjectFirst,
    ObjectKey,
    Colon,
    ObjectValue,
    ObjectAfter,
}
struct Frame {
    node: usize,
    expect: Expect,
    key: Option<Range<usize>>,
}

fn invalid(byte: u8, context: &str) -> GoText {
    let quoted = if byte == b'\'' {
        "'\\\''".to_owned()
    } else if byte == b'"' {
        "'\"'".to_owned()
    } else {
        let character = char::from(byte).to_string();
        let quoted = format_go_quoted_bytes(character.as_bytes());
        format!("'{}'", &quoted[1..quoted.len() - 1])
    };
    format!("invalid character {quoted} {context}").into()
}
fn space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}
fn next(bytes: &[u8], at: usize, context: &str) -> Result<u8, GoText> {
    bytes.get(at).copied().ok_or_else(|| {
        // Go's eof feeds one final space to the scalar scanner. A string
        // remains incomplete; numeric/literal states diagnose that space.
        if context == "in string literal" {
            "unexpected end of JSON input".into()
        } else {
            invalid(b' ', context)
        }
    })
}
fn string(bytes: &[u8], at: &mut usize) -> Result<(), GoText> {
    *at += 1;
    loop {
        let byte = next(bytes, *at, "in string literal")?;
        *at += 1;
        match byte {
            b'"' => return Ok(()),
            b'\\' => {
                let escaped = next(bytes, *at, "in string escape code")?;
                *at += 1;
                if escaped == b'u' {
                    for _ in 0..4 {
                        let digit = next(bytes, *at, "in \\u hexadecimal character escape")?;
                        if !digit.is_ascii_hexdigit() {
                            return Err(invalid(digit, "in \\u hexadecimal character escape"));
                        }
                        *at += 1;
                    }
                } else if !matches!(
                    escaped,
                    b'b' | b'f' | b'n' | b'r' | b't' | b'\\' | b'/' | b'"'
                ) {
                    return Err(invalid(escaped, "in string escape code"));
                }
            }
            0..=31 => return Err(invalid(byte, "in string literal")),
            _ => {}
        }
    }
}
fn number(bytes: &[u8], at: &mut usize) -> Result<(), GoText> {
    if bytes[*at] == b'-' {
        *at += 1;
        if !next(bytes, *at, "in numeric literal")?.is_ascii_digit() {
            return Err(invalid(bytes[*at], "in numeric literal"));
        }
    }
    if bytes[*at] == b'0' {
        *at += 1;
    } else {
        while bytes.get(*at).is_some_and(u8::is_ascii_digit) {
            *at += 1;
        }
    }
    if bytes.get(*at) == Some(&b'.') {
        *at += 1;
        let digit = next(bytes, *at, "after decimal point in numeric literal")?;
        if !digit.is_ascii_digit() {
            return Err(invalid(digit, "after decimal point in numeric literal"));
        }
        while bytes.get(*at).is_some_and(u8::is_ascii_digit) {
            *at += 1;
        }
    }
    if bytes.get(*at).is_some_and(|b| matches!(b, b'e' | b'E')) {
        *at += 1;
        if bytes.get(*at).is_some_and(|b| matches!(b, b'+' | b'-')) {
            *at += 1;
        }
        let digit = next(bytes, *at, "in exponent of numeric literal")?;
        if !digit.is_ascii_digit() {
            return Err(invalid(digit, "in exponent of numeric literal"));
        }
        while bytes.get(*at).is_some_and(u8::is_ascii_digit) {
            *at += 1;
        }
    }
    Ok(())
}

pub(super) fn scan(bytes: &[u8]) -> Result<Vec<Node>, GoText> {
    let mut nodes: Vec<Node> = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    let mut at = 0;
    loop {
        while bytes.get(at).is_some_and(|b| space(*b)) {
            at += 1;
        }
        let Some(&byte) = bytes.get(at) else {
            return if !nodes.is_empty() && stack.is_empty() {
                Ok(nodes)
            } else {
                Err("unexpected end of JSON input".into())
            };
        };
        if stack.is_empty() && !nodes.is_empty() {
            return Err(invalid(byte, "after top-level value"));
        }
        if let Some(frame) = stack.last_mut() {
            match frame.expect {
                Expect::ArrayFirst | Expect::ArrayAfter if byte == b']' => {
                    at += 1;
                    nodes[frame.node].span.end = at;
                    stack.pop();
                    continue;
                }
                Expect::ObjectFirst | Expect::ObjectAfter if byte == b'}' => {
                    at += 1;
                    nodes[frame.node].span.end = at;
                    stack.pop();
                    continue;
                }
                Expect::ArrayAfter | Expect::ObjectAfter => {
                    if byte != b',' {
                        return Err(invalid(
                            byte,
                            if matches!(frame.expect, Expect::ArrayAfter) {
                                "after array element"
                            } else {
                                "after object key:value pair"
                            },
                        ));
                    }
                    frame.expect = if matches!(frame.expect, Expect::ArrayAfter) {
                        Expect::ArrayValue
                    } else {
                        Expect::ObjectKey
                    };
                    at += 1;
                    continue;
                }
                Expect::ObjectFirst | Expect::ObjectKey => {
                    if byte != b'"' {
                        return Err(invalid(byte, "looking for beginning of object key string"));
                    }
                    let start = at;
                    string(bytes, &mut at)?;
                    frame.key = Some(start..at);
                    frame.expect = Expect::Colon;
                    continue;
                }
                Expect::Colon => {
                    if byte != b':' {
                        return Err(invalid(byte, "after object key"));
                    }
                    frame.expect = Expect::ObjectValue;
                    at += 1;
                    continue;
                }
                _ => {}
            }
        }
        let start = at;
        let parent = stack.last().map(|frame| frame.node);
        let key = stack.last_mut().and_then(|frame| {
            frame.expect = if matches!(frame.expect, Expect::ObjectValue) {
                Expect::ObjectAfter
            } else {
                Expect::ArrayAfter
            };
            frame.key.take()
        });
        let kind = match byte {
            b'[' => Kind::Array,
            b'{' => Kind::Object,
            b'"' => {
                string(bytes, &mut at)?;
                Kind::String
            }
            b'-' | b'0'..=b'9' => {
                number(bytes, &mut at)?;
                Kind::Number
            }
            b't' | b'f' | b'n' => {
                let literal: &[u8] = match byte {
                    b't' => b"true",
                    b'f' => b"false",
                    _ => b"null",
                };
                for &expected in &literal[1..] {
                    at += 1;
                    let context = format!(
                        "in literal {} (expecting '{}')",
                        std::str::from_utf8(literal).expect("ASCII literal"),
                        char::from(expected)
                    );
                    let actual = next(bytes, at, &context)?;
                    if actual != expected {
                        return Err(invalid(actual, &context));
                    }
                }
                at += 1;
                if byte == b'n' { Kind::Null } else { Kind::Bool }
            }
            _ => return Err(invalid(byte, "looking for beginning of value")),
        };
        let index = nodes.len();
        nodes.push(Node {
            kind,
            span: start..at,
            parent,
            key,
        });
        if matches!(kind, Kind::Array | Kind::Object) {
            at += 1;
            stack.push(Frame {
                node: index,
                key: None,
                expect: if kind == Kind::Array {
                    Expect::ArrayFirst
                } else {
                    Expect::ObjectFirst
                },
            });
            if stack.len() > 10_000 {
                return Err(invalid(byte, "exceeded max depth"));
            }
        }
    }
}
