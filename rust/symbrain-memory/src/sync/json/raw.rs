//! Iterative JSON grammar/ranges; no recursive allocation for ignored values.
// Source-derived compatibility logic: Go1.26.7 SDK, Go Authors, BSD-3-Clause.
// Retained license: scripts/memory-sync-oracle/reference/GO-SDK-LICENSE.

use super::super::SyncError;
use super::strings::string_end;

#[derive(Clone, Copy)]
pub(super) struct Raw<'a>(pub &'a [u8]);
impl<'a> Raw<'a> {
    pub fn null(self) -> bool {
        self.0 == b"null"
    }
    pub fn kind(self) -> &'static str {
        match self.0.first() {
            Some(b'"') => "string",
            Some(b'{') => "object",
            Some(b'[') => "array",
            Some(b't' | b'f') => "bool",
            Some(b'n') => "null",
            _ => "number",
        }
    }
    pub fn members(self) -> Result<Vec<(String, Raw<'a>)>, SyncError> {
        if self.0.first() != Some(&b'{') {
            return Err(SyncError("expected JSON object".into()));
        }
        let mut at = 1;
        let mut members = Vec::new();
        loop {
            at = space(self.0, at);
            if self.0.get(at) == Some(&b'}') {
                return Ok(members);
            }
            let end = string_end(self.0, at)?;
            let name = super::strings::unquote(&self.0[at..end])?;
            at = space(self.0, end) + 1;
            at = space(self.0, at);
            let end = scan(self.0, at)?;
            members.push((name, Raw(&self.0[at..end])));
            at = space(self.0, end);
            if self.0.get(at) == Some(&b',') {
                at += 1;
            } else {
                return Ok(members);
            }
        }
    }
    pub fn elements(self) -> Result<Vec<Raw<'a>>, SyncError> {
        if self.0.first() != Some(&b'[') {
            return Err(SyncError("expected JSON array".into()));
        }
        let mut at = 1;
        let mut elements = Vec::new();
        loop {
            at = space(self.0, at);
            if self.0.get(at) == Some(&b']') {
                return Ok(elements);
            }
            let end = scan(self.0, at)?;
            elements.push(Raw(&self.0[at..end]));
            at = space(self.0, end);
            if self.0.get(at) == Some(&b',') {
                at += 1;
            } else {
                return Ok(elements);
            }
        }
    }
}
pub(super) fn space(data: &[u8], mut at: usize) -> usize {
    while data
        .get(at)
        .is_some_and(|c| matches!(c, b' ' | b'\t' | b'\r' | b'\n'))
    {
        at += 1;
    }
    at
}
pub(super) fn bad(byte: u8, context: &str) -> SyncError {
    let shown = match byte {
        b'\n' => "'\\n'".into(),
        b'\r' => "'\\r'".into(),
        b'\t' => "'\\t'".into(),
        b'\'' => "'\\\''".into(),
        b'\\' => "'\\\\'".into(),
        0..=31 | 127 => format!("'\\x{byte:02x}'"),
        _ => format!("'{}'", char::from(byte)),
    };
    SyncError(format!("invalid character {shown} {context}"))
}
fn eof() -> SyncError {
    SyncError("unexpected EOF".into())
}

#[derive(Clone, Copy)]
enum Frame {
    Object(u8),
    Array(u8),
}

pub(super) fn first(data: &[u8], stream: bool) -> Result<Raw<'_>, SyncError> {
    let at = space(data, 0);
    if at == data.len() {
        return Err(SyncError(
            if stream {
                "EOF"
            } else {
                "unexpected end of JSON input"
            }
            .into(),
        ));
    }
    let end = scan(data, at).map_err(|error| {
        if !stream && error.0 == "unexpected EOF" {
            SyncError("unexpected end of JSON input".into())
        } else {
            error
        }
    })?;
    if !stream {
        let trailing = space(data, end);
        if let Some(byte) = data.get(trailing) {
            return Err(bad(*byte, "after top-level value"));
        }
    }
    Ok(Raw(&data[at..end]))
}

fn scan(data: &[u8], start: usize) -> Result<usize, SyncError> {
    let mut at = start;
    let mut stack: Vec<Frame> = Vec::new();
    let mut root_done = false;
    loop {
        at = space(data, at);
        if stack.is_empty() && root_done {
            return Ok(at);
        }
        let byte = *data.get(at).ok_or_else(eof)?;
        match stack.last().copied() {
            Some(Frame::Object(state @ (0 | 1))) => {
                if byte == b'}' && state == 0 {
                    at += 1;
                    stack.pop();
                    continue;
                }
                if byte != b'"' {
                    return Err(bad(byte, "looking for beginning of object key string"));
                }
                at = string_end(data, at)?;
                *stack.last_mut().expect("object") = Frame::Object(2);
                continue;
            }
            Some(Frame::Object(2)) => {
                if byte != b':' {
                    return Err(bad(byte, "after object key"));
                }
                at += 1;
                *stack.last_mut().expect("object") = Frame::Object(3);
                continue;
            }
            Some(Frame::Object(4)) => {
                match byte {
                    b',' => {
                        at += 1;
                        *stack.last_mut().expect("object") = Frame::Object(1);
                    }
                    b'}' => {
                        at += 1;
                        stack.pop();
                    }
                    _ => return Err(bad(byte, "after object key:value pair")),
                }
                continue;
            }
            Some(Frame::Array(0)) if byte == b']' => {
                at += 1;
                stack.pop();
                continue;
            }
            Some(Frame::Array(2)) => {
                match byte {
                    b',' => {
                        at += 1;
                        *stack.last_mut().expect("array") = Frame::Array(1);
                    }
                    b']' => {
                        at += 1;
                        stack.pop();
                    }
                    _ => return Err(bad(byte, "after array element")),
                }
                continue;
            }
            _ => {}
        }
        if let Some(frame) = stack.last_mut() {
            *frame = match *frame {
                Frame::Object(_) => Frame::Object(4),
                Frame::Array(_) => Frame::Array(2),
            };
        } else {
            root_done = true;
        }
        match byte {
            b'{' => {
                stack.push(Frame::Object(0));
                at += 1;
            }
            b'[' => {
                stack.push(Frame::Array(0));
                at += 1;
            }
            b'"' => at = string_end(data, at)?,
            b't' | b'f' | b'n' => {
                let literal: &[u8] = match byte {
                    b't' => b"true",
                    b'f' => b"false",
                    _ => b"null",
                };
                for (index, want) in literal.iter().enumerate() {
                    let found = *data.get(at + index).ok_or_else(eof)?;
                    if found != *want {
                        return Err(bad(
                            found,
                            &format!(
                                "in literal {} (expecting '{}')",
                                String::from_utf8_lossy(literal),
                                char::from(*want)
                            ),
                        ));
                    }
                }
                at += literal.len();
            }
            b'-' | b'0'..=b'9' => at = number_end(data, at)?,
            _ => return Err(bad(byte, "looking for beginning of value")),
        }
        if stack.len() > 10_000 {
            return Err(bad(byte, "exceeded max depth"));
        }
        if stack.is_empty() && root_done {
            // Go Decoder.readValue accepts the first scalar once the next
            // byte arrives, even when stateEndTop marks that byte invalid.
            // Whole-value Unmarshal checks trailing bytes separately.
            return Ok(at);
        }
    }
}

fn number_end(data: &[u8], mut at: usize) -> Result<usize, SyncError> {
    if data[at] == b'-' {
        at += 1;
    }
    let first = *data.get(at).ok_or_else(eof)?;
    match first {
        b'0' => at += 1,
        b'1'..=b'9' => {
            at += 1;
            while data.get(at).is_some_and(u8::is_ascii_digit) {
                at += 1;
            }
        }
        _ => return Err(bad(first, "in numeric literal")),
    }
    if data.get(at) == Some(&b'.') {
        at += 1;
        let byte = *data.get(at).ok_or_else(eof)?;
        if !byte.is_ascii_digit() {
            return Err(bad(byte, "after decimal point in numeric literal"));
        }
        while data.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
    }
    if data.get(at).is_some_and(|c| matches!(c, b'e' | b'E')) {
        at += 1;
        if data.get(at).is_some_and(|c| matches!(c, b'+' | b'-')) {
            at += 1;
        }
        let byte = *data.get(at).ok_or_else(eof)?;
        if !byte.is_ascii_digit() {
            return Err(bad(byte, "in exponent of numeric literal"));
        }
        while data.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
    }
    Ok(at)
}
