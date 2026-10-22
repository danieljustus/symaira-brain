//! Port of the `encoding/json` `Decoder.Token` flow used by Go's
//! `decodeJSONObject` (`internal/harness/ordered_map.go`).
//!
//! Every error message is the exact text Go's decoder produces for the same
//! bytes (without the `parse json: ` wrapper). The decoder is iterative: Go
//! recurses without a depth limit when it walks container tokens, so nesting
//! is bounded only by the configuration size cap, never by the native stack.

use crate::go_quote::{decode_rune, json_quote_char};
use crate::json_parser::{Object, Value};

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenState {
    TopValue,
    ArrayStart,
    ArrayValue,
    ArrayComma,
    ObjectStart,
    ObjectKey,
    ObjectColon,
    ObjectValue,
    ObjectComma,
}

enum Token {
    Delim(u8),
    Scalar(Scalar),
}

/// A decoded scalar; strings are kept apart because they double as keys.
enum Scalar {
    Text(String),
    Other(Value),
}

impl Scalar {
    fn into_value(self) -> Value {
        match self {
            Self::Text(text) => Value::String(text),
            Self::Other(value) => value,
        }
    }
}

impl Token {
    fn display(&self) -> String {
        match self {
            Self::Delim(byte) => char::from(*byte).to_string(),
            Self::Scalar(Scalar::Text(text) | Scalar::Other(Value::Number(text))) => text.clone(),
            Self::Scalar(Scalar::Other(Value::Bool(value))) => value.to_string(),
            Self::Scalar(Scalar::Other(_)) => "<nil>".to_owned(),
        }
    }
}

enum Frame {
    Object(Object, Option<String>),
    Array(Vec<Value>),
}

struct Decoder<'a> {
    input: &'a [u8],
    position: usize,
    state: TokenState,
    stack: Vec<TokenState>,
}

/// `bytes.TrimSpace(data)` is empty: only (Unicode) whitespace.
fn is_blank(data: &[u8]) -> bool {
    std::str::from_utf8(data).is_ok_and(|text| text.chars().all(char::is_whitespace))
}

/// Decodes a harness JSON config like Go's `decodeJSONObject`.
///
/// # Errors
/// Returns Go's error text (without the `parse json: ` wrapper).
pub(crate) fn decode_object(data: &[u8]) -> Result<Value, String> {
    if is_blank(data) {
        return Ok(Value::Object(Object::new()));
    }
    let mut decoder = Decoder {
        input: data,
        position: 0,
        state: TokenState::TopValue,
        stack: Vec::new(),
    };
    match decoder.token()? {
        Token::Delim(b'{') => {}
        other => return Err(format!("expected {{, got {}", other.display())),
    }
    let mut frames = vec![Frame::Object(Object::new(), None)];
    let root = loop {
        let wants_value = match frames.last_mut().expect("open frame") {
            Frame::Object(_, pending) => {
                if decoder.more() {
                    match decoder.token()? {
                        Token::Scalar(Scalar::Text(key)) => *pending = Some(key),
                        other => {
                            return Err(format!("expected string key, got {}", other.display()));
                        }
                    }
                    true
                } else {
                    false
                }
            }
            Frame::Array(_) => decoder.more(),
        };
        if !wants_value {
            let closing = decoder.token()?;
            if frames.len() == 1 && !matches!(closing, Token::Delim(b'}')) {
                return Err(format!("expected }}, got {}", closing.display()));
            }
            let value = match frames.pop().expect("open frame") {
                Frame::Object(object, _) => Value::Object(object),
                Frame::Array(values) => Value::Array(values),
            };
            if frames.is_empty() {
                break value;
            }
            attach(&mut frames, value);
            continue;
        }
        match decoder.token()? {
            Token::Delim(b'{') => frames.push(Frame::Object(Object::new(), None)),
            Token::Delim(b'[') => frames.push(Frame::Array(Vec::new())),
            Token::Delim(other) => {
                return Err(format!("unexpected delimiter: {}", char::from(other)));
            }
            Token::Scalar(scalar) => attach(&mut frames, scalar.into_value()),
        }
    };
    if decoder.more() {
        return Err("unexpected trailing content after the top-level value".to_owned());
    }
    Ok(root)
}

fn attach(frames: &mut [Frame], value: Value) {
    match frames.last_mut().expect("open frame") {
        Frame::Object(object, pending) => {
            let key = pending.take().unwrap_or_default();
            object.set(&key, value);
        }
        Frame::Array(values) => values.push(value),
    }
}

impl Decoder<'_> {
    fn peek(&mut self) -> Result<u8, String> {
        while let Some(&byte) = self.input.get(self.position) {
            if matches!(byte, b' ' | b'\t' | b'\r' | b'\n') {
                self.position += 1;
            } else {
                return Ok(byte);
            }
        }
        Err("EOF".to_owned())
    }

    fn more(&mut self) -> bool {
        self.peek().is_ok_and(|byte| byte != b']' && byte != b'}')
    }

    fn value_allowed(&self) -> bool {
        matches!(
            self.state,
            TokenState::TopValue
                | TokenState::ArrayStart
                | TokenState::ArrayValue
                | TokenState::ObjectValue
        )
    }

    fn value_end(&mut self) {
        match self.state {
            TokenState::ArrayStart | TokenState::ArrayValue => self.state = TokenState::ArrayComma,
            TokenState::ObjectValue => self.state = TokenState::ObjectComma,
            _ => {}
        }
    }

    fn token_error(&self, byte: u8) -> String {
        let context = match self.state {
            TokenState::TopValue
            | TokenState::ArrayStart
            | TokenState::ArrayValue
            | TokenState::ObjectValue => " looking for beginning of value",
            TokenState::ArrayComma => " after array element",
            TokenState::ObjectKey => " looking for beginning of object key string",
            TokenState::ObjectColon => " after object key",
            TokenState::ObjectComma => " after object key:value pair",
            // Go leaves the context empty for this state.
            TokenState::ObjectStart => "",
        };
        format!("invalid character {}{context}", json_quote_char(byte))
    }

    fn open(&mut self, delim: u8, next: TokenState) -> Result<Token, String> {
        if !self.value_allowed() {
            return Err(self.token_error(delim));
        }
        self.position += 1;
        self.stack.push(self.state);
        self.state = next;
        Ok(Token::Delim(delim))
    }

    fn close(&mut self, delim: u8, allowed: [TokenState; 2]) -> Result<Token, String> {
        if !allowed.contains(&self.state) {
            return Err(self.token_error(delim));
        }
        self.position += 1;
        self.state = self.stack.pop().unwrap_or(TokenState::TopValue);
        self.value_end();
        Ok(Token::Delim(delim))
    }

    fn token(&mut self) -> Result<Token, String> {
        loop {
            let byte = self.peek()?;
            match byte {
                b'[' => return self.open(byte, TokenState::ArrayStart),
                b']' => {
                    return self.close(byte, [TokenState::ArrayStart, TokenState::ArrayComma]);
                }
                b'{' => return self.open(byte, TokenState::ObjectStart),
                b'}' => {
                    return self.close(byte, [TokenState::ObjectStart, TokenState::ObjectComma]);
                }
                b':' => {
                    if self.state != TokenState::ObjectColon {
                        return Err(self.token_error(byte));
                    }
                    self.position += 1;
                    self.state = TokenState::ObjectValue;
                }
                b',' => match self.state {
                    TokenState::ArrayComma => {
                        self.position += 1;
                        self.state = TokenState::ArrayValue;
                    }
                    TokenState::ObjectComma => {
                        self.position += 1;
                        self.state = TokenState::ObjectKey;
                    }
                    _ => return Err(self.token_error(byte)),
                },
                b'"' if matches!(self.state, TokenState::ObjectStart | TokenState::ObjectKey) => {
                    let value = self.scalar()?;
                    self.state = TokenState::ObjectColon;
                    return Ok(Token::Scalar(value));
                }
                _ => {
                    if !self.value_allowed() {
                        return Err(self.token_error(byte));
                    }
                    let value = self.scalar()?;
                    self.value_end();
                    return Ok(Token::Scalar(value));
                }
            }
        }
    }

    /// `Decoder.Decode` of one scalar: Go's scanner errors, `unexpected EOF`.
    fn scalar(&mut self) -> Result<Scalar, String> {
        let start = self.position;
        match self.input[start] {
            b'"' => {
                let end = self.string_end(start)?;
                self.position = end;
                Ok(Scalar::Text(unquote(&self.input[start + 1..end - 1])))
            }
            b'-' | b'0'..=b'9' => {
                let end = self.number_end(start)?;
                self.position = end;
                Ok(Scalar::Other(Value::Number(
                    String::from_utf8_lossy(&self.input[start..end]).into_owned(),
                )))
            }
            b't' => self.literal(start, b"true", Value::Bool(true)),
            b'f' => self.literal(start, b"false", Value::Bool(false)),
            b'n' => self.literal(start, b"null", Value::Null),
            other => Err(format!(
                "invalid character {} looking for beginning of value",
                json_quote_char(other)
            )),
        }
    }

    fn at(&self, index: usize) -> Result<u8, String> {
        self.input
            .get(index)
            .copied()
            .ok_or_else(|| "unexpected EOF".to_owned())
    }

    fn bad<T>(&self, index: usize, context: &str) -> Result<T, String> {
        let byte = self.at(index)?;
        Err(format!(
            "invalid character {} {context}",
            json_quote_char(byte)
        ))
    }

    fn string_end(&self, start: usize) -> Result<usize, String> {
        let mut index = start + 1;
        loop {
            match self.at(index)? {
                b'"' => return Ok(index + 1),
                b'\\' => {
                    index += 1;
                    match self.at(index)? {
                        b'b' | b'f' | b'n' | b'r' | b't' | b'\\' | b'/' | b'"' => index += 1,
                        b'u' => {
                            index += 1;
                            for _ in 0..4 {
                                if !self.at(index)?.is_ascii_hexdigit() {
                                    return self.bad(index, "in \\u hexadecimal character escape");
                                }
                                index += 1;
                            }
                        }
                        _ => return self.bad(index, "in string escape code"),
                    }
                }
                0..=0x1f => return self.bad(index, "in string literal"),
                _ => index += 1,
            }
        }
    }

    fn digits(&self, mut index: usize) -> usize {
        while self.input.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        index
    }

    fn number_end(&self, start: usize) -> Result<usize, String> {
        let mut index = start;
        if self.at(index)? == b'-' {
            index += 1;
            if !self.at(index)?.is_ascii_digit() {
                return self.bad(index, "in numeric literal");
            }
        }
        if self.at(index)? == b'0' {
            index += 1;
        } else {
            index = self.digits(index);
        }
        if self.input.get(index) == Some(&b'.') {
            index += 1;
            if !self.at(index)?.is_ascii_digit() {
                return self.bad(index, "after decimal point in numeric literal");
            }
            index = self.digits(index);
        }
        if matches!(self.input.get(index), Some(b'e' | b'E')) {
            index += 1;
            if matches!(self.input.get(index), Some(b'+' | b'-')) {
                index += 1;
            }
            if !self.at(index)?.is_ascii_digit() {
                return self.bad(index, "in exponent of numeric literal");
            }
            index = self.digits(index);
        }
        Ok(index)
    }

    fn literal(&mut self, start: usize, text: &[u8], value: Value) -> Result<Scalar, String> {
        for (offset, &expected) in text.iter().enumerate().skip(1) {
            let index = start + offset;
            if self.at(index)? != expected {
                return self.bad(
                    index,
                    &format!(
                        "in literal {} (expecting '{}')",
                        String::from_utf8_lossy(text),
                        char::from(expected)
                    ),
                );
            }
        }
        self.position = start + text.len();
        Ok(Scalar::Other(value))
    }
}

fn hex4(bytes: &[u8]) -> Option<u32> {
    let digits = bytes.get(..4)?;
    let mut value = 0_u32;
    for &digit in digits {
        value = value * 16 + char::from(digit).to_digit(16)?;
    }
    Some(value)
}

/// Go's `unquote` for a syntactically valid JSON string body.
fn unquote(body: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(body)
        && body.iter().all(|&byte| byte != b'\\' && byte >= b' ')
    {
        return text.to_owned();
    }
    let mut out = String::with_capacity(body.len());
    let mut index = 0;
    while index < body.len() {
        let byte = body[index];
        if byte == b'\\' {
            index += 1;
            match body.get(index).copied() {
                Some(b'b') => out.push('\u{8}'),
                Some(b'f') => out.push('\u{c}'),
                Some(b'n') => out.push('\n'),
                Some(b'r') => out.push('\r'),
                Some(b't') => out.push('\t'),
                Some(escaped @ (b'"' | b'\\' | b'/')) => out.push(char::from(escaped)),
                Some(b'u') => {
                    let first = hex4(&body[index + 1..]).unwrap_or(0xFFFD);
                    index += 5;
                    let mut rune = first;
                    if (0xD800..0xE000).contains(&first) {
                        rune = 0xFFFD;
                        if body.get(index) == Some(&b'\\')
                            && body.get(index + 1) == Some(&b'u')
                            && let Some(second) = hex4(&body[index + 2..])
                            && (0xD800..0xDC00).contains(&first)
                            && (0xDC00..0xE000).contains(&second)
                        {
                            rune = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                            index += 6;
                        }
                    }
                    out.push(char::from_u32(rune).unwrap_or(char::REPLACEMENT_CHARACTER));
                    continue;
                }
                _ => {}
            }
            index += 1;
        } else if byte < 0x80 {
            out.push(char::from(byte));
            index += 1;
        } else {
            let (rune, width) = decode_rune(&body[index..]);
            out.push(rune);
            index += width;
        }
    }
    out
}
