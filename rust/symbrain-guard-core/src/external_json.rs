//! Syntax-only JSON validation with encoding/json diagnostics.
//!
//! Validate raw bytes before decoding fields: Go checks syntax before invoking
//! time.Time.UnmarshalJSON, ignores arbitrarily large unknown numbers, and
//! replaces invalid Unicode only inside strings. An explicit stack bounds depth
//! without growing the native call stack on attacker-controlled input.
use super::quote_go_char;

#[derive(Clone, Copy)]
enum State {
    Value,
    Key(bool),
    Colon,
    ObjectEnd,
    ArrayStart,
    ArrayEnd,
    End,
}

struct Scanner<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl Scanner<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }
    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.pos += 1;
        }
    }
    fn error(&self, context: &str) -> String {
        self.peek().map_or_else(
            || "unexpected end of JSON input".into(),
            |c| format!("invalid character {} {context}", quote_go_char(c)),
        )
    }
    fn string(&mut self) -> Result<(), String> {
        self.pos += 1;
        loop {
            match self.peek() {
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(());
                }
                Some(b'\\') => {
                    self.pos += 1;
                    match self.peek() {
                        Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => {
                            self.pos += 1;
                        }
                        Some(b'u') => {
                            self.pos += 1;
                            for _ in 0..4 {
                                if !self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                                    return Err(self.error("in \\u hexadecimal character escape"));
                                }
                                self.pos += 1;
                            }
                        }
                        _ => return Err(self.error("in string escape code")),
                    }
                }
                None | Some(0..=31) => return Err(self.error("in string literal")),
                Some(_) => self.pos += 1,
            }
        }
    }
    fn digits(&mut self, context: &str) -> Result<(), String> {
        if !self.peek().is_some_and(|c| c.is_ascii_digit()) {
            return Err(self.error(context));
        }
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        Ok(())
    }
    fn number(&mut self) -> Result<(), String> {
        if self.peek() == Some(b'-') {
            self.pos += 1;
            if !self.peek().is_some_and(|c| c.is_ascii_digit()) {
                return Err(self.error("in numeric literal"));
            }
        }
        if self.peek() == Some(b'0') {
            self.pos += 1;
        } else {
            self.digits("in numeric literal")?;
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            self.digits("after decimal point in numeric literal")?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            self.digits("in exponent of numeric literal")?;
        }
        Ok(())
    }
    fn literal(&mut self, text: &[u8]) -> Result<(), String> {
        for &wanted in text {
            if self.peek() != Some(wanted) {
                let name = std::str::from_utf8(text).expect("ASCII literal");
                return Err(self.error(&format!(
                    "in literal {name} (expecting {})",
                    quote_go_char(wanted)
                )));
            }
            self.pos += 1;
        }
        Ok(())
    }
}

pub(super) fn validate(input: &[u8]) -> Result<(), String> {
    let mut s = Scanner {
        bytes: input,
        pos: 0,
    };
    let mut stack = vec![State::End, State::Value];
    let mut depth = 0;
    while let Some(state) = stack.pop() {
        s.space();
        match state {
            State::Value => match s.peek() {
                Some(b'{') => {
                    depth += 1;
                    stack.push(State::Key(true));
                    s.pos += 1;
                }
                Some(b'[') => {
                    depth += 1;
                    stack.push(State::ArrayStart);
                    s.pos += 1;
                }
                Some(b'"') => s.string()?,
                Some(b'n') => s.literal(b"null")?,
                Some(b't') => s.literal(b"true")?,
                Some(b'f') => s.literal(b"false")?,
                Some(b'-' | b'0'..=b'9') => s.number()?,
                _ => return Err(s.error("looking for beginning of value")),
            },
            State::Key(first) => match s.peek() {
                Some(b'}') if first => {
                    s.pos += 1;
                    depth -= 1;
                }
                Some(b'"') => {
                    s.string()?;
                    stack.push(State::ObjectEnd);
                    stack.push(State::Value);
                    stack.push(State::Colon);
                }
                _ => return Err(s.error("looking for beginning of object key string")),
            },
            State::Colon => {
                if s.peek() != Some(b':') {
                    return Err(s.error("after object key"));
                }
                s.pos += 1;
            }
            State::ObjectEnd => match s.peek() {
                Some(b'}') => {
                    s.pos += 1;
                    depth -= 1;
                }
                Some(b',') => {
                    s.pos += 1;
                    stack.push(State::Key(false));
                }
                _ => return Err(s.error("after object key:value pair")),
            },
            State::ArrayStart => {
                if s.peek() == Some(b']') {
                    s.pos += 1;
                    depth -= 1;
                } else {
                    stack.push(State::ArrayEnd);
                    stack.push(State::Value);
                }
            }
            State::ArrayEnd => match s.peek() {
                Some(b']') => {
                    s.pos += 1;
                    depth -= 1;
                }
                Some(b',') => {
                    s.pos += 1;
                    stack.push(State::ArrayEnd);
                    stack.push(State::Value);
                }
                _ => return Err(s.error("after array element")),
            },
            State::End => {
                if s.peek().is_some() {
                    return Err(s.error("after top-level value"));
                }
            }
        }
        if depth > 10_000 {
            return Err(format!(
                "invalid character {} exceeded max depth",
                quote_go_char(input[s.pos - 1])
            ));
        }
    }
    Ok(())
}
