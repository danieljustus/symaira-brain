//! Go `encoding/json`-compatible decoding of MCP client configurations.
//!
//! Ports the JSON path of `CoreKit` `mcpcfgkit` (`parseJSONConfig`): a whole-input
//! syntax check with Go's exact `SyntaxError` wording, then a document-order
//! decode into the `Entry` struct with Go's field matching (exact, then
//! case-folded), duplicate-key, `null` and first-`UnmarshalTypeError` rules.

use std::collections::BTreeMap;

use super::guard_scan_config::Entry;

/// Go's `maxNestingDepth`.
const MAX_DEPTH: usize = 10_000;

#[derive(Debug)]
pub(crate) enum Json {
    Null,
    Bool,
    Number,
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

/// Dropping a 10 000-level document recursively would overflow small (Windows
/// main-thread, test-thread) stacks, so children are released iteratively.
impl Drop for Json {
    fn drop(&mut self) {
        let mut pending: Vec<Json> = Vec::new();
        take_children(self, &mut pending);
        while let Some(mut node) = pending.pop() {
            take_children(&mut node, &mut pending);
        }
    }
}

fn take_children(node: &mut Json, into: &mut Vec<Json>) {
    match node {
        Json::Array(items) => into.append(items),
        Json::Object(items) => into.extend(items.drain(..).map(|(_, value)| value)),
        _ => {}
    }
}

impl Json {
    fn kind(&self) -> &'static str {
        match self {
            Json::Null => "null",
            Json::Bool => "bool",
            Json::Number => "number",
            Json::String(_) => "string",
            Json::Array(_) => "array",
            Json::Object(_) => "object",
        }
    }
}

/// Go's `[]rune(string(data))` then `string(runes)`: every invalid byte becomes
/// its own U+FFFD (Rust's lossy conversion merges truncated sequences).
pub(crate) fn go_lossy(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len());
    let mut rest = data;
    loop {
        match std::str::from_utf8(rest) {
            Ok(valid) => {
                out.push_str(valid);
                return out;
            }
            Err(error) => {
                let (valid, tail) = rest.split_at(error.valid_up_to());
                out.push_str(std::str::from_utf8(valid).unwrap_or_default());
                out.push('\u{fffd}');
                rest = &tail[1..];
            }
        }
    }
}

/// `quoteChar` from `encoding/json/scanner.go` for one input byte.
fn quote_char(byte: u8) -> String {
    if byte == b'\'' {
        return "'\\''".to_owned();
    }
    if byte == b'"' {
        return "'\"'".to_owned();
    }
    let inner = match byte {
        b'\\' => "\\\\".to_owned(),
        7 => "\\a".to_owned(),
        8 => "\\b".to_owned(),
        9 => "\\t".to_owned(),
        10 => "\\n".to_owned(),
        11 => "\\v".to_owned(),
        12 => "\\f".to_owned(),
        13 => "\\r".to_owned(),
        0x20..=0x7e | 0xa1..=0xac | 0xae..=0xff => char::from(byte).to_string(),
        0..=0x1f | 0x7f => format!("\\x{byte:02x}"),
        _ => format!("\\u{byte:04x}"),
    };
    format!("'{inner}'")
}

const EOF_MESSAGE: &str = "unexpected end of JSON input";

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

enum Frame {
    Object(Vec<(String, Json)>),
    Array(Vec<Json>),
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while matches!(self.bytes.get(self.pos), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.pos += 1;
        }
    }

    fn fail<T>(&self, context: &str) -> Result<T, String> {
        match self.bytes.get(self.pos) {
            Some(&byte) => Err(format!("invalid character {} {context}", quote_char(byte))),
            None => Err(EOF_MESSAGE.to_owned()),
        }
    }

    fn peek(&self) -> Result<u8, String> {
        self.bytes
            .get(self.pos)
            .copied()
            .ok_or_else(|| EOF_MESSAGE.to_owned())
    }

    fn key(&mut self) -> Result<String, String> {
        self.skip_ws();
        if self.peek()? != b'"' {
            return self.fail("looking for beginning of object key string");
        }
        let key = self.string()?;
        self.skip_ws();
        if self.peek()? != b':' {
            return self.fail("after object key");
        }
        self.pos += 1;
        Ok(key)
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let mut value = 0;
        for _ in 0..4 {
            let byte = self.peek()?;
            let Some(digit) = char::from(byte).to_digit(16) else {
                return self.fail("in \\u hexadecimal character escape");
            };
            value = value * 16 + digit;
            self.pos += 1;
        }
        Ok(value)
    }

    fn string(&mut self) -> Result<String, String> {
        self.pos += 1; // opening quote
        let mut out = String::new();
        loop {
            let byte = self.peek()?;
            match byte {
                b'"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.pos += 1;
                    let escape = self.peek()?;
                    self.pos += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let unit = self.hex4()?;
                            out.push(self.unicode(unit));
                        }
                        _ => {
                            self.pos -= 1;
                            return self.fail("in string escape code");
                        }
                    }
                }
                0..=0x1f => return self.fail("in string literal"),
                _ => {
                    let start = self.pos;
                    while self
                        .bytes
                        .get(self.pos)
                        .is_some_and(|b| !matches!(b, b'"' | b'\\' | 0..=0x1f))
                    {
                        self.pos += 1;
                    }
                    out.push_str(
                        std::str::from_utf8(&self.bytes[start..self.pos]).unwrap_or("\u{fffd}"),
                    );
                }
            }
        }
    }

    /// `\uXXXX` after the first four digits were read, with Go's surrogate rules:
    /// a valid pair combines; a lone surrogate becomes U+FFFD and consumes nothing more.
    fn unicode(&mut self, unit: u32) -> char {
        if (0xd800..0xdc00).contains(&unit) && self.bytes[self.pos..].starts_with(b"\\u") {
            let save = self.pos;
            self.pos += 2;
            if let Ok(low) = self.hex4()
                && (0xdc00..0xe000).contains(&low)
            {
                let combined = 0x10000 + ((unit - 0xd800) << 10) + (low - 0xdc00);
                return char::from_u32(combined).unwrap_or('\u{fffd}');
            }
            self.pos = save;
        }
        char::from_u32(unit).unwrap_or('\u{fffd}')
    }

    fn digits(&mut self) {
        while self.bytes.get(self.pos).is_some_and(u8::is_ascii_digit) {
            self.pos += 1;
        }
    }

    fn need_digit(&mut self, context: &str) -> Result<(), String> {
        if self.peek()?.is_ascii_digit() {
            self.pos += 1;
            Ok(())
        } else {
            self.fail(context)
        }
    }

    fn number(&mut self) -> Result<(), String> {
        if self.bytes[self.pos] == b'-' {
            self.pos += 1;
            if !self.peek()?.is_ascii_digit() {
                return self.fail("in numeric literal");
            }
        }
        if self.bytes[self.pos] == b'0' {
            self.pos += 1;
        } else {
            self.digits();
        }
        if self.bytes.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            self.need_digit("after decimal point in numeric literal")?;
            self.digits();
        }
        if matches!(self.bytes.get(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.bytes.get(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            self.need_digit("in exponent of numeric literal")?;
            self.digits();
        }
        Ok(())
    }

    fn literal(&mut self, word: &'static str) -> Result<(), String> {
        self.pos += 1;
        for expected in word.bytes().skip(1) {
            if self.peek()? != expected {
                return self.fail(&format!(
                    "in literal {word} (expecting '{}')",
                    char::from(expected)
                ));
            }
            self.pos += 1;
        }
        Ok(())
    }

    /// Parses one complete document with Go's `checkValid` ordering of errors.
    fn document(&mut self) -> Result<Json, String> {
        let mut stack: Vec<Frame> = Vec::new();
        let mut pending_key: Vec<String> = Vec::new();
        'value: loop {
            self.skip_ws();
            let mut done = match self.peek()? {
                b'{' | b'[' => {
                    if stack.len() + 1 > MAX_DEPTH {
                        return self.fail("exceeded max depth");
                    }
                    let open = self.bytes[self.pos];
                    self.pos += 1;
                    self.skip_ws();
                    if open == b'{' {
                        if self.peek()? == b'}' {
                            self.pos += 1;
                            Json::Object(Vec::new())
                        } else {
                            let key = self.key()?;
                            pending_key.push(key);
                            stack.push(Frame::Object(Vec::new()));
                            continue 'value;
                        }
                    } else if self.peek()? == b']' {
                        self.pos += 1;
                        Json::Array(Vec::new())
                    } else {
                        stack.push(Frame::Array(Vec::new()));
                        continue 'value;
                    }
                }
                b'"' => Json::String(self.string()?),
                b'-' | b'0'..=b'9' => {
                    self.number()?;
                    Json::Number
                }
                b't' => {
                    self.literal("true")?;
                    Json::Bool
                }
                b'f' => {
                    self.literal("false")?;
                    Json::Bool
                }
                b'n' => {
                    self.literal("null")?;
                    Json::Null
                }
                _ => return self.fail("looking for beginning of value"),
            };
            loop {
                match stack.last_mut() {
                    None => {
                        self.skip_ws();
                        if self.pos < self.bytes.len() {
                            return self.fail("after top-level value");
                        }
                        return Ok(done);
                    }
                    Some(Frame::Object(items)) => {
                        let key = pending_key.pop().unwrap_or_default();
                        items.push((key, done));
                        self.skip_ws();
                        match self.peek()? {
                            b',' => {
                                self.pos += 1;
                                let key = self.key()?;
                                pending_key.push(key);
                                continue 'value;
                            }
                            b'}' => {
                                self.pos += 1;
                                let Some(Frame::Object(items)) = stack.pop() else {
                                    unreachable!("object frame")
                                };
                                done = Json::Object(items);
                            }
                            _ => return self.fail("after object key:value pair"),
                        }
                    }
                    Some(Frame::Array(items)) => {
                        items.push(done);
                        self.skip_ws();
                        match self.peek()? {
                            b',' => {
                                self.pos += 1;
                                continue 'value;
                            }
                            b']' => {
                                self.pos += 1;
                                let Some(Frame::Array(items)) = stack.pop() else {
                                    unreachable!("array frame")
                                };
                                done = Json::Array(items);
                            }
                            _ => return self.fail("after array element"),
                        }
                    }
                }
            }
        }
    }
}

/// Validates and parses `text` the way `json.Unmarshal` does before decoding.
pub(crate) fn parse(text: &str) -> Result<Json, String> {
    Parser {
        bytes: text.as_bytes(),
        pos: 0,
    }
    .document()
}

/// Outcome of the JSON attempt: entries, a key miss (falls back to YAML) or an error.
pub(crate) enum Outcome {
    Entries(BTreeMap<String, Entry>),
    KeyNotFound,
    Error(String),
}

/// `parseJSONConfig`: `text` is the JSONC-stripped, untrimmed document.
pub(crate) fn entries(text: &str, key: &str) -> Outcome {
    let root = match parse(text) {
        Ok(root) => root,
        Err(error) => return Outcome::Error(format!("parse JSON: {error}")),
    };
    // The caller only gets here for text starting with `{`; a valid document
    // is therefore an object. `map[string]RawMessage`: the last duplicate wins.
    let Json::Object(items) = &root else {
        return Outcome::Error("parse JSON: not an object".to_owned());
    };
    let found = [key, "mcpServers", "mcp_servers", "mcp"]
        .into_iter()
        .find_map(|candidate| {
            items
                .iter()
                .rev()
                .find(|(name, _)| name == candidate)
                .map(|(_, value)| (candidate, value))
        });
    let Some((found, value)) = found else {
        return Outcome::KeyNotFound;
    };
    match decode_entries(value) {
        Ok(map) => Outcome::Entries(map),
        Err(error) => Outcome::Error(format!("parse {found:?}: {error}")),
    }
}

const ENTRY_TYPE: &str = "mcpcfgkit.Entry";

fn type_error(value: &Json, target: &str) -> String {
    format!(
        "json: cannot unmarshal {} into Go value of type {target}",
        value.kind()
    )
}

fn field_error(value: &Json, field: &str, target: &str) -> String {
    format!(
        "json: cannot unmarshal {} into Go struct field Entry.{field} of type {target}",
        value.kind()
    )
}

/// `unicode.SimpleFold`-equivalent for the ASCII field names: the long s and the
/// Kelvin sign fold into ASCII letters, everything else folds by ASCII case.
fn fold(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '\u{17f}' => 'S',
            '\u{212a}' => 'K',
            c => c.to_ascii_uppercase(),
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Command,
    Args,
    Url,
    Type,
    Transport,
    Env,
    Environment,
}

const FIELDS: [(&str, Field); 7] = [
    ("command", Field::Command),
    ("args", Field::Args),
    ("url", Field::Url),
    ("type", Field::Type),
    ("transport", Field::Transport),
    ("env", Field::Env),
    ("environment", Field::Environment),
];

fn match_field(key: &str) -> Option<(&'static str, Field)> {
    FIELDS
        .iter()
        .find(|(name, _)| *name == key)
        .or_else(|| {
            let folded = fold(key);
            FIELDS.iter().find(|(name, _)| fold(name) == folded)
        })
        .copied()
}

fn decode_entries(value: &Json) -> Result<BTreeMap<String, Entry>, String> {
    let mut first_error: Option<String> = None;
    let mut entries = BTreeMap::new();
    match value {
        Json::Null => {}
        Json::Object(items) => {
            for (name, value) in items {
                let mut entry = Entry::default();
                decode_entry(value, &mut entry, &mut first_error);
                entries.insert(name.clone(), entry);
            }
        }
        other => return Err(type_error(other, &format!("map[string]{ENTRY_TYPE}"))),
    }
    first_error.map_or(Ok(entries), Err)
}

fn decode_entry(value: &Json, entry: &mut Entry, first_error: &mut Option<String>) {
    let mut fail = |message: String| {
        first_error.get_or_insert(message);
    };
    let items = match value {
        Json::Null => return,
        Json::Object(items) => items,
        other => return fail(type_error(other, ENTRY_TYPE)),
    };
    for (key, value) in items {
        let Some((name, field)) = match_field(key) else {
            continue;
        };
        match field {
            Field::Command | Field::Url | Field::Type | Field::Transport => {
                let slot = match field {
                    Field::Command => &mut entry.command,
                    Field::Url => &mut entry.url,
                    Field::Type => &mut entry.kind,
                    _ => &mut entry.transport_name,
                };
                match value {
                    Json::Null => {}
                    Json::String(text) => slot.clone_from(text),
                    other => fail(field_error(other, name, "string")),
                }
            }
            Field::Args => match value {
                Json::Null => entry.args.clear(),
                Json::Array(items) => {
                    entry.args.clear();
                    for item in items {
                        match item {
                            Json::Null => entry.args.push(String::new()),
                            Json::String(text) => entry.args.push(text.clone()),
                            other => fail(field_error(other, name, "string")),
                        }
                    }
                }
                other => fail(field_error(other, name, "[]string")),
            },
            Field::Env | Field::Environment => {
                let slot = if field == Field::Env {
                    &mut entry.env
                } else {
                    &mut entry.environment
                };
                match value {
                    Json::Null => slot.clear(),
                    Json::Object(items) => {
                        for (key, value) in items {
                            match value {
                                Json::Null => {
                                    slot.insert(key.clone(), String::new());
                                }
                                Json::String(text) => {
                                    slot.insert(key.clone(), text.clone());
                                }
                                other => fail(field_error(other, name, "string")),
                            }
                        }
                    }
                    other => fail(field_error(other, name, "map[string]string")),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(text: &str) -> String {
        parse(text).expect_err("syntax error")
    }

    #[test]
    fn syntax_errors_use_go_wording() {
        assert_eq!(error("{"), EOF_MESSAGE);
        assert_eq!(
            error("{\"a\":c}"),
            "invalid character 'c' looking for beginning of value"
        );
        assert_eq!(
            error("{'a':1}"),
            "invalid character '\\'' looking for beginning of object key string"
        );
        assert_eq!(
            error("{\"a\":1} x"),
            "invalid character 'x' after top-level value"
        );
        assert_eq!(
            error("{\"a\":01}"),
            "invalid character '1' after object key:value pair"
        );
        assert_eq!(
            error("{\"a\":1.}"),
            "invalid character '}' after decimal point in numeric literal"
        );
        assert_eq!(
            error("{\"a\":\"\\q\"}"),
            "invalid character 'q' in string escape code"
        );
        assert_eq!(
            error("{\"a\":tru}"),
            "invalid character '}' in literal true (expecting 'e')"
        );
        assert_eq!(
            error("{\"a\":\"\\u12g4\"}"),
            "invalid character 'g' in \\u hexadecimal character escape"
        );
        assert_eq!(
            error("{\"a\":\u{7f}}"),
            "invalid character '\\x7f' looking for beginning of value"
        );
        assert_eq!(
            error("{\"a\":\u{e9}}"),
            "invalid character 'Ã' looking for beginning of value"
        );
    }

    #[test]
    fn lone_surrogates_and_pairs_follow_go() {
        fn string_of(text: &str) -> String {
            match parse(text).unwrap() {
                Json::String(ref value) => value.clone(),
                _ => panic!("not a string"),
            }
        }
        assert_eq!(string_of("\"\\ud800\""), "\u{fffd}");
        assert_eq!(string_of("\"\\ud83d\\ude00\""), "\u{1f600}");
        assert_eq!(string_of("\"\\ud83dx\\ude00\""), "\u{fffd}x\u{fffd}");
    }

    #[test]
    fn go_lossy_replaces_each_invalid_byte() {
        assert_eq!(go_lossy(b"a\xe2\x82c"), "a\u{fffd}\u{fffd}c");
    }

    #[test]
    fn depth_limit_matches_go() {
        let deep = format!("{}0{}", "[".repeat(10_001), "]".repeat(10_001));
        assert_eq!(error(&deep), "invalid character '[' exceeded max depth");
        let fine = format!("{}0{}", "[".repeat(10_000), "]".repeat(10_000));
        assert!(parse(&fine).is_ok());
    }
}
