//! `encoding/json` decoding of the two shapes the memory store hydrates:
//! `map[string]string` (metadata) and `[]float32` (embeddings), with the
//! diagnostics Go prints for malformed and wrongly typed input.
//!
//! Go validates the whole document first (`checkValid`, a byte-level state
//! machine) and only then decodes, saving the first type mismatch while it
//! keeps going. Both phases are ported exactly: syntax errors win over type
//! errors, a type error never stops the walk, and `null` leaves the target
//! untouched (a nil map or slice).

use std::collections::BTreeMap;

/// `strconv.IsPrint` for the Latin-1 range, the only one `quoteChar` can see.
fn latin1_printable(code: u32) -> bool {
    matches!(code, 0x20..=0x7e | 0xa1..=0xac | 0xae..=0xff)
}

/// `quoteChar` of `encoding/json`: `strconv.Quote` of the byte as a rune,
/// without the surrounding quotes, wrapped in apostrophes.
fn quote_char(byte: u8) -> String {
    match byte {
        b'\'' => return "'\\''".to_owned(),
        b'"' => return "'\"'".to_owned(),
        _ => {}
    }
    let code = u32::from(byte);
    let body = match byte {
        0x07 => "\\a".to_owned(),
        0x08 => "\\b".to_owned(),
        0x0c => "\\f".to_owned(),
        b'\n' => "\\n".to_owned(),
        b'\r' => "\\r".to_owned(),
        b'\t' => "\\t".to_owned(),
        0x0b => "\\v".to_owned(),
        b'\\' => "\\\\".to_owned(),
        _ if code < 0x20 || code == 0x7f => format!("\\x{code:02x}"),
        _ if code < 0x80 || latin1_printable(code) => {
            char::from_u32(code).map(String::from).unwrap_or_default()
        }
        _ => format!("\\u{code:04x}"),
    };
    format!("'{body}'")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    BeginValueOrEmpty,
    BeginValue,
    BeginStringOrEmpty,
    BeginString,
    EndValue,
    EndTop,
    InString,
    InStringEsc,
    InStringEscU,
    InStringEscU1,
    InStringEscU12,
    InStringEscU123,
    Neg,
    Zero,
    One,
    Dot,
    Dot0,
    E,
    ESign,
    E0,
    T,
    Tr,
    Tru,
    F,
    Fa,
    Fal,
    Fals,
    N,
    Nu,
    Nul,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Parse {
    ObjectKey,
    ObjectValue,
    ArrayValue,
}

struct Scanner {
    state: State,
    stack: Vec<Parse>,
    end_top: bool,
    error: Option<String>,
}

const MAX_NESTING: usize = 10_000;

fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

impl Scanner {
    fn new() -> Self {
        Self {
            state: State::BeginValue,
            stack: Vec::new(),
            end_top: false,
            error: None,
        }
    }

    fn fail(&mut self, byte: u8, context: &str) {
        self.error = Some(format!("invalid character {} {context}", quote_char(byte)));
    }

    fn push(&mut self, parse: Parse) {
        self.stack.push(parse);
        if self.stack.len() > MAX_NESTING {
            self.error = Some("exceeded max depth".to_owned());
        }
    }

    fn value_done(&mut self) {
        self.state = State::EndValue;
    }

    /// One step of Go's scanner; `self.error` is set on failure.
    fn step(&mut self, byte: u8) {
        use State::{
            BeginString, BeginStringOrEmpty, BeginValue, BeginValueOrEmpty, Dot, Dot0, E, E0,
            ESign, EndTop, EndValue, F, Fa, Fal, Fals, InString, InStringEsc, InStringEscU,
            InStringEscU1, InStringEscU12, InStringEscU123, N, Neg, Nu, Nul, One, T, Tr, Tru, Zero,
        };
        match self.state {
            BeginValueOrEmpty => {
                if is_space(byte) {
                    return;
                }
                if byte == b']' {
                    self.state = EndValue;
                    self.step(byte);
                    return;
                }
                self.state = BeginValue;
                self.step(byte);
            }
            BeginValue => {
                if is_space(byte) {
                    return;
                }
                match byte {
                    b'{' => {
                        self.state = BeginStringOrEmpty;
                        self.push(Parse::ObjectKey);
                    }
                    b'[' => {
                        self.state = BeginValueOrEmpty;
                        self.push(Parse::ArrayValue);
                    }
                    b'"' => self.state = InString,
                    b'-' => self.state = Neg,
                    b'0' => self.state = Zero,
                    b'1'..=b'9' => self.state = One,
                    b't' => self.state = T,
                    b'f' => self.state = F,
                    b'n' => self.state = N,
                    _ => self.fail(byte, "looking for beginning of value"),
                }
            }
            BeginStringOrEmpty => {
                if is_space(byte) {
                    return;
                }
                if byte == b'}' {
                    if let Some(top) = self.stack.last_mut() {
                        *top = Parse::ObjectValue;
                    }
                    self.state = EndValue;
                    self.step(byte);
                    return;
                }
                self.state = BeginString;
                self.step(byte);
            }
            BeginString => {
                if is_space(byte) {
                    return;
                }
                if byte == b'"' {
                    self.state = InString;
                    return;
                }
                self.fail(byte, "looking for beginning of object key string");
            }
            EndValue => {
                let Some(top) = self.stack.last().copied() else {
                    self.state = EndTop;
                    self.end_top = true;
                    self.step(byte);
                    return;
                };
                if is_space(byte) {
                    return;
                }
                match top {
                    Parse::ObjectKey => {
                        if byte == b':' {
                            if let Some(last) = self.stack.last_mut() {
                                *last = Parse::ObjectValue;
                            }
                            self.state = BeginValue;
                        } else {
                            self.fail(byte, "after object key");
                        }
                    }
                    Parse::ObjectValue => {
                        if byte == b',' {
                            if let Some(last) = self.stack.last_mut() {
                                *last = Parse::ObjectKey;
                            }
                            self.state = BeginString;
                        } else if byte == b'}' {
                            self.stack.pop();
                            self.after_close();
                        } else {
                            self.fail(byte, "after object key:value pair");
                        }
                    }
                    Parse::ArrayValue => {
                        if byte == b',' {
                            self.state = BeginValue;
                        } else if byte == b']' {
                            self.stack.pop();
                            self.after_close();
                        } else {
                            self.fail(byte, "after array element");
                        }
                    }
                }
            }
            EndTop => {
                if !is_space(byte) {
                    self.fail(byte, "after top-level value");
                }
            }
            InString => match byte {
                b'"' => self.state = EndValue,
                b'\\' => self.state = InStringEsc,
                _ if byte < 0x20 => self.fail(byte, "in string literal"),
                _ => {}
            },
            InStringEsc => match byte {
                b'b' | b'f' | b'n' | b'r' | b't' | b'\\' | b'/' | b'"' => self.state = InString,
                b'u' => self.state = InStringEscU,
                _ => self.fail(byte, "in string escape code"),
            },
            InStringEscU | InStringEscU1 | InStringEscU12 | InStringEscU123 => {
                if byte.is_ascii_hexdigit() {
                    self.state = match self.state {
                        InStringEscU => InStringEscU1,
                        InStringEscU1 => InStringEscU12,
                        InStringEscU12 => InStringEscU123,
                        _ => InString,
                    };
                } else {
                    self.fail(byte, "in \\u hexadecimal character escape");
                }
            }
            Neg => match byte {
                b'0' => self.state = Zero,
                b'1'..=b'9' => self.state = One,
                _ => self.fail(byte, "in numeric literal"),
            },
            One => {
                if byte.is_ascii_digit() {
                    return;
                }
                self.state = Zero;
                self.step(byte);
            }
            Zero => match byte {
                b'.' => self.state = Dot,
                b'e' | b'E' => self.state = E,
                _ => {
                    self.state = EndValue;
                    self.step(byte);
                }
            },
            Dot => {
                if byte.is_ascii_digit() {
                    self.state = Dot0;
                } else {
                    self.fail(byte, "after decimal point in numeric literal");
                }
            }
            Dot0 => match byte {
                _ if byte.is_ascii_digit() => {}
                b'e' | b'E' => self.state = E,
                _ => {
                    self.state = EndValue;
                    self.step(byte);
                }
            },
            E => {
                if byte == b'+' || byte == b'-' {
                    self.state = ESign;
                } else {
                    self.state = ESign;
                    self.step(byte);
                }
            }
            ESign => {
                if byte.is_ascii_digit() {
                    self.state = E0;
                } else {
                    self.fail(byte, "in exponent of numeric literal");
                }
            }
            E0 => {
                if !byte.is_ascii_digit() {
                    self.state = EndValue;
                    self.step(byte);
                }
            }
            T => self.literal(byte, b'r', Tr, "true", "'r'"),
            Tr => self.literal(byte, b'u', Tru, "true", "'u'"),
            Tru => self.literal_end(byte, b'e', "true", "'e'"),
            F => self.literal(byte, b'a', Fa, "false", "'a'"),
            Fa => self.literal(byte, b'l', Fal, "false", "'l'"),
            Fal => self.literal(byte, b's', Fals, "false", "'s'"),
            Fals => self.literal_end(byte, b'e', "false", "'e'"),
            N => self.literal(byte, b'u', Nu, "null", "'u'"),
            Nu => self.literal(byte, b'l', Nul, "null", "'l'"),
            Nul => self.literal_end(byte, b'l', "null", "'l'"),
        }
    }

    fn after_close(&mut self) {
        self.value_done();
    }

    fn literal(&mut self, byte: u8, expect: u8, next: State, name: &str, shown: &str) {
        if byte == expect {
            self.state = next;
        } else {
            self.fail(byte, &format!("in literal {name} (expecting {shown})"));
        }
    }

    fn literal_end(&mut self, byte: u8, expect: u8, name: &str, shown: &str) {
        if byte == expect {
            self.state = State::EndValue;
        } else {
            self.fail(byte, &format!("in literal {name} (expecting {shown})"));
        }
    }
}

/// `checkValid`: the exact `SyntaxError` text, or `Ok`.
pub(crate) fn check_valid(data: &[u8]) -> Result<(), String> {
    let mut scanner = Scanner::new();
    for byte in data {
        scanner.step(*byte);
        if let Some(error) = scanner.error.take() {
            return Err(error);
        }
    }
    // `eof`: feed one trailing space unless the document already ended.
    if !scanner.end_top {
        scanner.step(b' ');
        if scanner.end_top {
            return Ok(());
        }
        return Err(scanner
            .error
            .take()
            .unwrap_or_else(|| "unexpected end of JSON input".to_owned()));
    }
    Ok(())
}

/// A shallow view of one validated JSON value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Null,
    Bool,
    Number,
    String,
    Array,
    Object,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool => "bool",
            Self::Number => "number",
            Self::String => "string",
            Self::Array => "array",
            Self::Object => "object",
        }
    }
}

struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn skip_space(&mut self) {
        while self.at < self.data.len() && is_space(self.data[self.at]) {
            self.at += 1;
        }
    }

    fn peek_kind(&mut self) -> Kind {
        self.skip_space();
        match self.data[self.at] {
            b'{' => Kind::Object,
            b'[' => Kind::Array,
            b'"' => Kind::String,
            b't' | b'f' => Kind::Bool,
            b'n' => Kind::Null,
            _ => Kind::Number,
        }
    }

    /// Returns the raw bytes of a string literal including the quotes.
    fn raw_string(&mut self) -> &'a [u8] {
        let start = self.at;
        self.at += 1;
        while self.data[self.at] != b'"' {
            if self.data[self.at] == b'\\' {
                self.at += 1;
            }
            self.at += 1;
        }
        self.at += 1;
        &self.data[start..self.at]
    }

    fn raw_scalar(&mut self) -> &'a [u8] {
        let start = self.at;
        while self.at < self.data.len()
            && !is_space(self.data[self.at])
            && !matches!(self.data[self.at], b',' | b']' | b'}' | b':')
        {
            self.at += 1;
        }
        &self.data[start..self.at]
    }

    /// Skips one validated value of any kind.
    fn skip_value(&mut self) {
        self.skip_space();
        match self.data[self.at] {
            b'"' => {
                self.raw_string();
            }
            b'{' | b'[' => {
                let mut depth = 0_usize;
                loop {
                    let byte = self.data[self.at];
                    match byte {
                        b'"' => {
                            self.raw_string();
                            continue;
                        }
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => {
                            depth -= 1;
                            if depth == 0 {
                                self.at += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    self.at += 1;
                }
            }
            _ => {
                self.raw_scalar();
            }
        }
    }
}

/// `unquoteBytes` of `encoding/json`: escapes resolved, every invalid UTF-8
/// byte becomes U+FFFD.
fn unquote(raw: &[u8]) -> String {
    let body = &raw[1..raw.len() - 1];
    let mut out = String::with_capacity(body.len());
    let mut index = 0;
    while index < body.len() {
        let byte = body[index];
        if byte == b'\\' {
            let escape = body[index + 1];
            index += 2;
            match escape {
                b'b' => out.push('\u{8}'),
                b'f' => out.push('\u{c}'),
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                b'u' => {
                    let first = hex4(&body[index..]);
                    index += 4;
                    let mut code = first;
                    if (0xd800..0xe000).contains(&first) {
                        // A valid pair consumes the second escape too.
                        let paired =
                            body.get(index..index + 2) == Some(b"\\u") && body.len() >= index + 6;
                        let second = if paired { hex4(&body[index + 2..]) } else { 0 };
                        if (0xd800..0xdc00).contains(&first) && (0xdc00..0xe000).contains(&second) {
                            code = 0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00);
                            index += 6;
                        } else {
                            code = 0xfffd;
                        }
                    }
                    out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                }
                other => out.push(char::from(other)),
            }
        } else if byte < 0x80 {
            out.push(char::from(byte));
            index += 1;
        } else {
            let width = utf8_width(&body[index..]);
            match width {
                Some(width) => {
                    out.push_str(std::str::from_utf8(&body[index..index + width]).unwrap_or(""));
                    index += width;
                }
                None => {
                    out.push('\u{fffd}');
                    index += 1;
                }
            }
        }
    }
    out
}

fn hex4(bytes: &[u8]) -> u32 {
    bytes[..4].iter().fold(0, |acc, byte| {
        acc * 16 + char::from(*byte).to_digit(16).unwrap_or(0)
    })
}

fn utf8_width(bytes: &[u8]) -> Option<usize> {
    for width in 2..=4 {
        if bytes.len() >= width && std::str::from_utf8(&bytes[..width]).is_ok() {
            return Some(width);
        }
    }
    None
}

/// A saved type error, in `UnmarshalTypeError` wording.
fn type_error(value: &str, target: &str) -> String {
    format!("json: cannot unmarshal {value} into Go value of type {target}")
}

/// Decodes into a `map[string]string`. `Ok(None)` is a nil map (JSON null).
pub fn decode_string_map(data: &[u8]) -> Result<Option<BTreeMap<String, String>>, String> {
    const TARGET: &str = "map[string]string";
    check_valid(data)?;
    let mut cursor = Cursor { data, at: 0 };
    let mut saved: Option<String> = None;
    match cursor.peek_kind() {
        Kind::Null => return Ok(None),
        Kind::Object => {}
        other => return Err(type_error(other.name(), TARGET)),
    }
    let mut map = BTreeMap::new();
    cursor.at += 1;
    loop {
        cursor.skip_space();
        match cursor.data[cursor.at] {
            b'}' => break,
            b',' => {
                cursor.at += 1;
                continue;
            }
            _ => {}
        }
        let key = unquote(cursor.raw_string());
        cursor.skip_space();
        cursor.at += 1; // ':'
        let kind = cursor.peek_kind();
        let value = match kind {
            Kind::String => unquote(cursor.raw_string()),
            Kind::Null => {
                cursor.skip_value();
                String::new()
            }
            other => {
                cursor.skip_value();
                saved.get_or_insert_with(|| type_error(other.name(), "string"));
                String::new()
            }
        };
        map.insert(key, value);
    }
    saved.map_or(Ok(Some(map)), Err)
}

/// Decodes into a `[]float32`. `Ok(None)` is a nil slice (JSON null).
pub fn decode_f32_vec(data: &[u8]) -> Result<Option<Vec<f32>>, String> {
    const TARGET: &str = "[]float32";
    check_valid(data)?;
    let mut cursor = Cursor { data, at: 0 };
    let mut saved: Option<String> = None;
    match cursor.peek_kind() {
        Kind::Null => return Ok(None),
        Kind::Array => {}
        other => return Err(type_error(other.name(), TARGET)),
    }
    let mut values = Vec::new();
    cursor.at += 1;
    loop {
        cursor.skip_space();
        match cursor.data[cursor.at] {
            b']' => break,
            b',' => {
                cursor.at += 1;
                continue;
            }
            _ => {}
        }
        let kind = cursor.peek_kind();
        match kind {
            Kind::Number => {
                let literal = cursor.raw_scalar();
                let text = std::str::from_utf8(literal).unwrap_or("0");
                match text.parse::<f32>() {
                    Ok(value) if value.is_finite() => values.push(value),
                    _ => {
                        saved.get_or_insert_with(|| {
                            type_error(&format!("number {text}"), "float32")
                        });
                        values.push(0.0);
                    }
                }
            }
            Kind::Null => {
                cursor.skip_value();
                values.push(0.0);
            }
            other => {
                cursor.skip_value();
                saved.get_or_insert_with(|| type_error(other.name(), "float32"));
                values.push(0.0);
            }
        }
    }
    saved.map_or(Ok(Some(values)), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_error(text: &str) -> String {
        decode_string_map(text.as_bytes()).expect_err("must fail")
    }

    #[test]
    fn syntax_errors_use_go_wording() {
        assert_eq!(
            map_error("broken"),
            "invalid character 'b' looking for beginning of value"
        );
        assert_eq!(map_error(""), "unexpected end of JSON input");
        assert_eq!(map_error("{\"a\":"), "unexpected end of JSON input");
        assert_eq!(
            map_error("tru"),
            "invalid character ' ' in literal true (expecting 'e')"
        );
        assert_eq!(
            map_error("{} x"),
            "invalid character 'x' after top-level value"
        );
        assert_eq!(
            map_error("{1:2}"),
            "invalid character '1' looking for beginning of object key string"
        );
        assert_eq!(
            map_error("{\"a\" 1}"),
            "invalid character '1' after object key"
        );
        assert_eq!(
            map_error("[1 2]"),
            "invalid character '2' after array element"
        );
        assert_eq!(
            map_error("\"\\x\""),
            "invalid character 'x' in string escape code"
        );
        assert_eq!(map_error("-"), "invalid character ' ' in numeric literal");
        assert_eq!(
            map_error("\u{e9}"),
            "invalid character '\u{c3}' looking for beginning of value"
        );
    }

    #[test]
    fn type_errors_follow_the_first_mismatch() {
        assert_eq!(
            map_error("[]"),
            "json: cannot unmarshal array into Go value of type map[string]string"
        );
        assert_eq!(
            map_error("false"),
            "json: cannot unmarshal bool into Go value of type map[string]string"
        );
        assert_eq!(
            map_error("{\"n\":1}"),
            "json: cannot unmarshal number into Go value of type string"
        );
        assert_eq!(
            map_error("{\"n\":1,\"n\":\"later\"}"),
            "json: cannot unmarshal number into Go value of type string"
        );
        assert!(decode_string_map(b"null").unwrap().is_none());
        assert_eq!(
            decode_string_map(b"{\"n\":null}").unwrap().unwrap()["n"],
            ""
        );
        assert_eq!(
            decode_f32_vec(b"[1e39]").unwrap_err(),
            "json: cannot unmarshal number 1e39 into Go value of type float32"
        );
        assert_eq!(
            decode_f32_vec(b"[\"x\"]").unwrap_err(),
            "json: cannot unmarshal string into Go value of type float32"
        );
        assert_eq!(decode_f32_vec(b"[null]").unwrap().unwrap(), vec![0.0]);
        assert_eq!(decode_f32_vec(b"[]").unwrap().unwrap(), Vec::<f32>::new());
    }

    #[test]
    fn strings_resolve_escapes_and_invalid_utf8() {
        let map = decode_string_map(b"{\"k\\u00e9\":\"\\ud83d\\ude00 \xff\\ud800\"}")
            .unwrap()
            .unwrap();
        assert_eq!(map["k\u{e9}"], "\u{1f600} \u{fffd}\u{fffd}");
    }
}
