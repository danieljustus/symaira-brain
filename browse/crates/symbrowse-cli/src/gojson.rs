//! A validating JSON reader that reproduces the diagnostics of Go's
//! `encoding/json` for user supplied documents (`set headers`, baselines).

/// Kind of a JSON value in `UnmarshalTypeError` wording.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Null,
    Bool,
    Number,
    String,
    Array,
    Object,
}

impl Kind {
    pub fn name(self) -> &'static str {
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

/// Result of scanning a document.
pub struct Scan {
    pub top: Kind,
    /// Top-level object members in document order: key and value kind.
    pub members: Vec<(String, Kind)>,
}

struct Scanner<'a> {
    bytes: &'a [u8],
    pos: usize,
}

fn quote_char(text: &str, pos: usize) -> String {
    let c = text.get(pos..).and_then(|rest| rest.chars().next()).unwrap_or('?');
    match c {
        '\'' => "'\\''".to_owned(),
        '"' => "'\"'".to_owned(),
        '\n' => "'\\n'".to_owned(),
        '\r' => "'\\r'".to_owned(),
        '\t' => "'\\t'".to_owned(),
        c if (c as u32) < 0x20 => format!("'\\x{:02x}'", c as u32),
        c => format!("'{c}'"),
    }
}

impl Scanner<'_> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && matches!(self.bytes[self.pos], b' ' | b'\t' | b'\r' | b'\n') {
            self.pos += 1;
        }
    }

    fn fail(&self, text: &str, context: &str) -> String {
        if self.pos >= self.bytes.len() {
            "unexpected end of JSON input".to_owned()
        } else {
            format!("invalid character {} {context}", quote_char(text, self.pos))
        }
    }

    fn digit_at(&self) -> bool {
        self.bytes.get(self.pos).is_some_and(u8::is_ascii_digit)
    }

    fn digits(&mut self) {
        while self.digit_at() {
            self.pos += 1;
        }
    }

    fn value(&mut self, text: &str) -> Result<Kind, String> {
        self.skip_ws();
        let Some(&byte) = self.bytes.get(self.pos) else {
            return Err("unexpected end of JSON input".into());
        };
        match byte {
            b'{' => {
                self.pos += 1;
                self.object(text, None)?;
                Ok(Kind::Object)
            }
            b'[' => {
                self.pos += 1;
                self.array(text)?;
                Ok(Kind::Array)
            }
            b'"' => {
                self.string(text)?;
                Ok(Kind::String)
            }
            b't' => self.literal(text, "true", Kind::Bool),
            b'f' => self.literal(text, "false", Kind::Bool),
            b'n' => self.literal(text, "null", Kind::Null),
            b'-' | b'0'..=b'9' => {
                self.number(text)?;
                Ok(Kind::Number)
            }
            _ => Err(self.fail(text, "looking for beginning of value")),
        }
    }

    fn literal(&mut self, text: &str, word: &str, kind: Kind) -> Result<Kind, String> {
        for (index, expected) in word.bytes().enumerate() {
            if self.bytes.get(self.pos) == Some(&expected) {
                self.pos += 1;
            } else {
                debug_assert!(index > 0, "a literal starts with its first byte");
                let context = format!("in literal {word} (expecting '{}')", expected as char);
                return Err(self.fail(text, &context));
            }
        }
        Ok(kind)
    }

    fn number(&mut self, text: &str) -> Result<(), String> {
        if self.bytes[self.pos] == b'-' {
            self.pos += 1;
            if !self.digit_at() {
                return Err(self.fail(text, "in numeric literal"));
            }
        }
        if self.bytes[self.pos] == b'0' {
            self.pos += 1;
        } else {
            self.digits();
        }
        if self.bytes.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            if !self.digit_at() {
                return Err(self.fail(text, "after decimal point in numeric literal"));
            }
            self.digits();
        }
        if matches!(self.bytes.get(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.bytes.get(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !self.digit_at() {
                return Err(self.fail(text, "in exponent of numeric literal"));
            }
            self.digits();
        }
        Ok(())
    }

    fn string(&mut self, text: &str) -> Result<String, String> {
        self.pos += 1;
        let mut out = Vec::new();
        loop {
            let Some(&byte) = self.bytes.get(self.pos) else {
                return Err("unexpected end of JSON input".into());
            };
            match byte {
                b'"' => {
                    self.pos += 1;
                    return Ok(String::from_utf8_lossy(&out).into_owned());
                }
                b'\\' => {
                    self.pos += 1;
                    let Some(&escape) = self.bytes.get(self.pos) else {
                        return Err("unexpected end of JSON input".into());
                    };
                    match escape {
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'"' | b'\\' | b'/' => out.push(escape),
                        b'u' => {
                            let mut code = 0u32;
                            for _ in 0..4 {
                                self.pos += 1;
                                match self.bytes.get(self.pos).and_then(|b| (*b as char).to_digit(16)) {
                                    Some(digit) => code = code * 16 + digit,
                                    None => {
                                        return Err(self.fail(text, "in \\u hexadecimal character escape"));
                                    }
                                }
                            }
                            let c = char::from_u32(code).unwrap_or('\u{fffd}');
                            let mut buffer = [0u8; 4];
                            out.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
                        }
                        _ => return Err(self.fail(text, "in string escape code")),
                    }
                    self.pos += 1;
                }
                0..=0x1f => return Err(self.fail(text, "in string literal")),
                _ => {
                    out.push(byte);
                    self.pos += 1;
                }
            }
        }
    }

    fn array(&mut self, text: &str) -> Result<(), String> {
        self.skip_ws();
        if self.bytes.get(self.pos) == Some(&b']') {
            self.pos += 1;
            return Ok(());
        }
        loop {
            self.value(text)?;
            self.skip_ws();
            match self.bytes.get(self.pos) {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => return Err(self.fail(text, "after array element")),
            }
        }
    }

    fn object(&mut self, text: &str, mut members: Option<&mut Vec<(String, Kind)>>) -> Result<(), String> {
        self.skip_ws();
        if self.bytes.get(self.pos) == Some(&b'}') {
            self.pos += 1;
            return Ok(());
        }
        loop {
            self.skip_ws();
            if self.bytes.get(self.pos) != Some(&b'"') {
                return Err(self.fail(text, "looking for beginning of object key string"));
            }
            let key = self.string(text)?;
            self.skip_ws();
            if self.bytes.get(self.pos) != Some(&b':') {
                return Err(self.fail(text, "after object key"));
            }
            self.pos += 1;
            let kind = self.value(text)?;
            if let Some(list) = members.as_deref_mut() {
                list.push((key, kind));
            }
            self.skip_ws();
            match self.bytes.get(self.pos) {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => return Err(self.fail(text, "after object key:value pair")),
            }
        }
    }
}

/// Validates `text` like `json.Unmarshal` and reports the top-level shape.
pub fn scan(text: &str) -> Result<Scan, String> {
    let mut scanner = Scanner {
        bytes: text.as_bytes(),
        pos: 0,
    };
    scanner.skip_ws();
    let mut members = Vec::new();
    let top = if scanner.bytes.get(scanner.pos) == Some(&b'{') {
        scanner.pos += 1;
        scanner.object(text, Some(&mut members))?;
        Kind::Object
    } else {
        scanner.value(text)?
    };
    scanner.skip_ws();
    if scanner.pos < scanner.bytes.len() {
        return Err(scanner.fail(text, "after top-level value"));
    }
    Ok(Scan { top, members })
}
