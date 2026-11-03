//! Go's `encoding/json` scanner and string handling for the sync client:
//! syntax errors with the shipped wording, `Decoder.Decode`'s "first value"
//! reading, and `unquote`.

/// `maxNestingDepth` of `encoding/json`.
const MAX_DEPTH: usize = 10_000;

/// Why a document does not scan.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Fail {
    /// The input ended inside a value.
    Eof,
    /// A syntax error, in `SyntaxError.Error()` wording.
    Syntax(String),
}

struct Scanner<'a> {
    input: &'a [u8],
    at: usize,
    depth: usize,
}

type Step = Result<(), Fail>;

impl Scanner<'_> {
    fn peek(&self) -> Option<u8> {
        self.input.get(self.at).copied()
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.at += 1;
        }
    }

    fn bad(&self, context: &str) -> Fail {
        match self.peek() {
            None => Fail::Eof,
            Some(byte) => Fail::Syntax(format!(
                "invalid character {} {context}",
                quote_char(&self.input[self.at..], byte)
            )),
        }
    }

    fn enter(&mut self) -> Step {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.bad("exceeded max depth"));
        }
        Ok(())
    }

    fn value(&mut self) -> Step {
        match self.peek() {
            None => Err(Fail::Eof),
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string(),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(b't') => self.literal("true"),
            Some(b'f') => self.literal("false"),
            Some(b'n') => self.literal("null"),
            Some(_) => Err(self.bad("looking for beginning of value")),
        }
    }

    fn literal(&mut self, word: &str) -> Step {
        self.at += 1;
        for expected in word.bytes().skip(1) {
            match self.peek() {
                None => return Err(Fail::Eof),
                Some(byte) if byte == expected => self.at += 1,
                Some(_) => {
                    return Err(self.bad(&format!(
                        "in literal {word} (expecting {})",
                        quote_char(&[expected], expected)
                    )));
                }
            }
        }
        Ok(())
    }

    fn object(&mut self) -> Step {
        self.enter()?;
        self.at += 1;
        self.space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            self.depth -= 1;
            return Ok(());
        }
        loop {
            self.space();
            if self.peek() != Some(b'"') {
                return Err(self.bad("looking for beginning of object key string"));
            }
            self.string()?;
            self.space();
            if self.peek() != Some(b':') {
                return Err(self.bad("after object key"));
            }
            self.at += 1;
            self.space();
            self.value()?;
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    self.depth -= 1;
                    return Ok(());
                }
                _ => return Err(self.bad("after object key:value pair")),
            }
        }
    }

    fn array(&mut self) -> Step {
        self.enter()?;
        self.at += 1;
        self.space();
        if self.peek() == Some(b']') {
            self.at += 1;
            self.depth -= 1;
            return Ok(());
        }
        loop {
            self.space();
            self.value()?;
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    self.depth -= 1;
                    return Ok(());
                }
                _ => return Err(self.bad("after array element")),
            }
        }
    }

    fn string(&mut self) -> Step {
        self.at += 1;
        loop {
            match self.peek() {
                None => return Err(Fail::Eof),
                Some(b'"') => {
                    self.at += 1;
                    return Ok(());
                }
                Some(b'\\') => {
                    let backslash = self.at;
                    self.at += 1;
                    match self.peek() {
                        None => return Err(Fail::Eof),
                        Some(b'b' | b'f' | b'n' | b'r' | b't' | b'\\' | b'/' | b'"') => {
                            self.at += 1;
                        }
                        Some(b'u') => {
                            self.at += 1;
                            for _ in 0..4 {
                                match self.peek() {
                                    None => return Err(Fail::Eof),
                                    Some(byte) if byte.is_ascii_hexdigit() => self.at += 1,
                                    Some(_) => {
                                        let from = backslash + 2;
                                        let to = (from + 4).min(self.input.len());
                                        return Err(Fail::Syntax(format!(
                                            "invalid escape sequence `\\u{}` in string",
                                            String::from_utf8_lossy(&self.input[from..to])
                                        )));
                                    }
                                }
                            }
                        }
                        Some(byte) => {
                            return Err(Fail::Syntax(format!(
                                "invalid escape sequence `\\{}` in string",
                                char::from(byte)
                            )));
                        }
                    }
                }
                Some(byte) if byte < 0x20 => return Err(self.bad("in string")),
                Some(_) => self.at += 1,
            }
        }
    }

    fn digits(&mut self) -> Step {
        if !matches!(self.peek(), Some(b'0'..=b'9')) {
            return Err(self.bad("in numeric literal"));
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        Ok(())
    }

    fn number(&mut self) -> Step {
        if self.peek() == Some(b'-') {
            self.at += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.bad("in numeric literal"));
            }
        }
        if self.peek() == Some(b'0') {
            self.at += 1;
        } else {
            self.digits()?;
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            self.digits()?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            self.digits()?;
        }
        Ok(())
    }
}

/// `quoteChar` of `encoding/json`.
fn quote_char(rest: &[u8], byte: u8) -> String {
    match byte {
        b'\'' => "'\\''".to_owned(),
        b'"' => "'\"'".to_owned(),
        _ => {
            let character = std::str::from_utf8(rest)
                .ok()
                .or_else(|| std::str::from_utf8(&rest[..rest.len().min(4)]).ok())
                .and_then(|text| text.chars().next())
                .unwrap_or(char::from(byte));
            let quoted = go_quote(&character.to_string());
            format!("'{}'", &quoted[1..quoted.len() - 1])
        }
    }
}

/// `strconv.Quote` for the characters that occur in these diagnostics.
#[must_use]
pub fn go_quote(text: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\u{b}' => out.push_str("\\v"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c if (0x80..=0xa0).contains(&(c as u32)) || c as u32 == 0xad => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The end of the first JSON value of `input` (`Decoder.readValue`): what
/// follows it is not looked at.
pub(crate) fn first_value(input: &[u8]) -> Result<usize, Fail> {
    let mut scanner = Scanner {
        input,
        at: 0,
        depth: 0,
    };
    scanner.space();
    scanner.value()?;
    Ok(scanner.at)
}

/// `checkValid`: the whole of `input` is exactly one value.
///
/// # Errors
/// The `SyntaxError` text; running out of input is "unexpected end of JSON input".
pub(crate) fn check_valid(input: &[u8]) -> Result<(), String> {
    let mut scanner = Scanner {
        input,
        at: 0,
        depth: 0,
    };
    let outcome = (|| {
        scanner.space();
        scanner.value()?;
        scanner.space();
        if scanner.peek().is_some() {
            return Err(scanner.bad("after top-level value"));
        }
        Ok(())
    })();
    outcome.map_err(|fail| match fail {
        Fail::Eof => "unexpected end of JSON input".to_owned(),
        Fail::Syntax(text) => text,
    })
}

/// Go's replacement of invalid UTF-8: every offending byte becomes U+FFFD.
pub(crate) fn go_lossy(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut rest = bytes;
    loop {
        match std::str::from_utf8(rest) {
            Ok(text) => {
                out.push_str(text);
                return out;
            }
            Err(error) => {
                let (valid, tail) = rest.split_at(error.valid_up_to());
                out.push_str(std::str::from_utf8(valid).unwrap_or_default());
                let bad = error.error_len().unwrap_or(tail.len());
                for _ in 0..bad {
                    out.push('\u{fffd}');
                }
                rest = &tail[bad..];
            }
        }
    }
}

fn hex4(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 4 {
        return None;
    }
    let mut value = 0_u32;
    for &byte in &bytes[..4] {
        value = value * 16 + char::from(byte).to_digit(16)?;
    }
    Some(value)
}

/// `unquoteBytes` for a token already known to be a valid JSON string
/// (`token` includes the quotes).
pub(crate) fn unquote(token: &[u8]) -> String {
    let inner = &token[1..token.len() - 1];
    let mut out: Vec<u8> = Vec::with_capacity(inner.len());
    let mut at = 0;
    while at < inner.len() {
        let byte = inner[at];
        if byte != b'\\' {
            out.push(byte);
            at += 1;
            continue;
        }
        at += 1;
        let escape = inner[at];
        at += 1;
        match escape {
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'u' => {
                let unit = hex4(&inner[at..]).unwrap_or(0xfffd);
                at += 4;
                let mut scalar = char::from_u32(unit);
                if (0xd800..0xe000).contains(&unit) {
                    scalar = None;
                    if (0xd800..0xdc00).contains(&unit)
                        && inner.get(at) == Some(&b'\\')
                        && inner.get(at + 1) == Some(&b'u')
                        && let Some(low) = hex4(&inner[at + 2..])
                        && (0xdc00..0xe000).contains(&low)
                    {
                        scalar = char::from_u32(0x10000 + ((unit - 0xd800) << 10) + (low - 0xdc00));
                        at += 6;
                    }
                }
                let mut buffer = [0_u8; 4];
                out.extend_from_slice(
                    scalar
                        .unwrap_or('\u{fffd}')
                        .encode_utf8(&mut buffer)
                        .as_bytes(),
                );
            }
            other => out.push(other),
        }
    }
    go_lossy(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_wording_follows_encoding_json() {
        let cases: [(&[u8], &str); 8] = [
            (b"{", "unexpected end of JSON input"),
            (b"", "unexpected end of JSON input"),
            (b"{\"a\":1,}", "invalid character '}' looking for beginning of object key string"),
            (b"{\"a\" 1}", "invalid character '1' after object key"),
            (b"[1 2]", "invalid character '2' after array element"),
            (b"nulx", "invalid character 'x' in literal null (expecting 'l')"),
            (b"{} {}", "invalid character '{' after top-level value"),
            (b"-x", "invalid character 'x' in numeric literal"),
        ];
        for (input, expected) in cases {
            assert_eq!(check_valid(input).unwrap_err(), expected);
        }
        assert!(check_valid(b" {\"a\":[1,2.5e3,null,true,\"\\u00e4\"]} ").is_ok());
    }

    #[test]
    fn the_first_value_ends_the_read() {
        assert_eq!(first_value(b"  {} trailing").unwrap(), 4);
        assert_eq!(first_value(b"123abc").unwrap(), 3);
        assert_eq!(first_value(b"tru"), Err(Fail::Eof));
        assert_eq!(first_value(b"   "), Err(Fail::Eof));
    }

    #[test]
    fn unquote_matches_go_for_surrogates_and_bad_bytes() {
        assert_eq!(unquote(br#""a\u00e4\n""#), "aä\n");
        assert_eq!(unquote(br#""\ud83d\ude00""#), "\u{1f600}");
        assert_eq!(unquote(br#""\ud83d""#), "\u{fffd}");
        assert_eq!(unquote(br#""\ude00x""#), "\u{fffd}x");
        assert_eq!(unquote(b"\"a\xe2\x82b\""), "a\u{fffd}\u{fffd}b");
    }
}
