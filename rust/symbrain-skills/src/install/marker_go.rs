//! Go-exact classification of an installed skill's `.symskills.json` marker.
//!
//! `install.Status` reads the marker with `os.ReadFile` and `json.Unmarshal`
//! into `Marker`: syntax is validated first (so a syntax error leaves the
//! marker empty), unknown schema versions are accepted, a mistyped field keeps
//! the fields decoded around it and only the first `UnmarshalTypeError` is
//! reported. Candidate scanner and diagnostics reviewed from the #813 branch.

use std::path::Path;

use cap_std::fs::Dir;

use super::marker::{MARKER_FILE, Marker};
use crate::SkillError;

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
    fn value(&mut self, stack: &mut Vec<State>, depth: &mut usize) -> Result<(), String> {
        match self.peek() {
            Some(byte @ (b'{' | b'[')) => {
                *depth += 1;
                stack.push(if byte == b'{' {
                    State::Key(true)
                } else {
                    State::ArrayStart
                });
                self.pos += 1;
            }
            Some(b'"') => self.string()?,
            Some(b'n') => self.literal(b"null")?,
            Some(b't') => self.literal(b"true")?,
            Some(b'f') => self.literal(b"false")?,
            Some(b'-' | b'0'..=b'9') => self.number()?,
            _ => return Err(self.error("looking for beginning of value")),
        }
        Ok(())
    }

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

type RawFields<'a> = Vec<(&'a [u8], &'a [u8])>;

fn fields(input: &[u8]) -> Result<RawFields<'_>, String> {
    let mut s = Scanner {
        bytes: input,
        pos: 0,
    };
    let mut stack = vec![State::End, State::Value];
    let mut depth = 0;
    let mut fields = Vec::new();
    let mut key = (0, 0);
    let mut value_start = 0;
    while let Some(state) = stack.pop() {
        s.space();
        if depth == 1 && matches!(state, State::Value) {
            value_start = s.pos;
        }
        if depth == 1 && matches!(state, State::ObjectEnd) {
            fields.push((&input[key.0..key.1], &input[value_start..s.pos]));
        }
        match state {
            State::Value => s.value(&mut stack, &mut depth)?,
            State::Key(first) => match s.peek() {
                Some(b'}') if first => {
                    s.pos += 1;
                    depth -= 1;
                }
                Some(b'"') => {
                    let key_start = s.pos;
                    s.string()?;
                    if depth == 1 {
                        key = (key_start, s.pos);
                    }
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
    Ok(fields)
}

pub(super) fn quote_go_char(byte: u8) -> String {
    let escaped = match byte {
        b'\'' => "\\'".to_owned(),
        b'"' => "\"".to_owned(),
        b'\\' => "\\\\".to_owned(),
        0x07 => "\\a".to_owned(),
        0x08 => "\\b".to_owned(),
        0x0c => "\\f".to_owned(),
        b'\n' => "\\n".to_owned(),
        b'\r' => "\\r".to_owned(),
        b'\t' => "\\t".to_owned(),
        0x0b => "\\v".to_owned(),
        // Go's quoteChar calls strconv.Quote(string(c)); IsPrint excludes
        // Latin-1 spacing U+00A0 and soft hyphen U+00AD.
        0x20..=0x7e | 0xa1..=0xac | 0xae..=0xff => char::from(byte).to_string(),
        0x80..=0xa0 | 0xad => format!("\\u{byte:04x}"),
        _ => format!("\\x{byte:02x}"),
    };
    format!("'{escaped}'")
}

fn hex_quad(bytes: &[u8]) -> Option<u16> {
    let text = std::str::from_utf8(bytes).ok()?;
    u16::from_str_radix(text, 16).ok()
}

// encoding/json replaces each invalid UTF-8 byte and unpaired UTF-16 surrogate
// inside strings with U+FFFD. Preserve escapes and valid surrogate pairs so the
// normal JSON parser still rejects malformed syntax and field types.
pub(super) fn repair_json_strings(raw: &[u8]) -> String {
    let mut utf8 = String::new();
    let mut remaining = raw;
    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(text) => {
                utf8.push_str(text);
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                utf8.push_str(std::str::from_utf8(&remaining[..valid]).unwrap_or_default());
                utf8.push('\u{fffd}');
                remaining = &remaining[valid + 1..];
            }
        }
    }
    let bytes = utf8.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let (mut i, mut in_string) = (0, false);
    while i < bytes.len() {
        if bytes[i] == b'"' {
            in_string = !in_string;
        }
        if in_string && bytes[i] == b'\\' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'u'
                && i + 6 <= bytes.len()
                && let Some(code) = hex_quad(&bytes[i + 2..i + 6])
                && (0xd800..=0xdfff).contains(&code)
            {
                if code <= 0xdbff
                    && i + 12 <= bytes.len()
                    && &bytes[i + 6..i + 8] == b"\\u"
                    && hex_quad(&bytes[i + 8..i + 12])
                        .is_some_and(|v| (0xdc00..=0xdfff).contains(&v))
                {
                    result.extend_from_slice(&bytes[i..i + 12]);
                    i += 12;
                } else {
                    result.extend_from_slice(b"\\ufffd");
                    i += 6;
                }
                continue;
            }
            result.extend_from_slice(&bytes[i..i + 2]);
            i += 2;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(result).expect("only ASCII escapes were replaced in UTF-8")
}

/// A marker decoded the way Go's `json.Unmarshal` fills `Marker`.
pub(super) struct Observation {
    pub(super) marker: Marker,
    pub(super) error: Option<String>,
}

/// Reads the marker below `root`; `None` when the file is absent.
pub(super) fn read(root: &Dir) -> Option<Observation> {
    match root.symlink_metadata(MARKER_FILE) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            return Some(Observation {
                marker: empty(),
                error: Some(error.to_string()),
            });
        }
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Some(Observation {
                marker: empty(),
                error: Some("marker is a symlink".to_owned()),
            });
        }
        Ok(_) => {}
    }
    // A marker the confined reader refuses (too large, not a regular file)
    // is a rejected input, reported on the row like any unreadable marker.
    match crate::load::read_limited_nofollow(
        root,
        Path::new(MARKER_FILE),
        "marker",
        crate::MAX_INPUT_SIZE,
    ) {
        Ok(bytes) => Some(decode(&bytes)),
        Err(error) => Some(Observation {
            marker: empty(),
            error: Some(error.to_string()),
        }),
    }
}

/// Opens `path` as a marker directory without following links and reads its
/// marker; an absent directory carries none.
pub(super) fn read_at_path(path: &Path) -> Result<Option<Observation>, SkillError> {
    let root = match super::replace::open_existing_dir(path) {
        Ok(root) => root,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(SkillError(format!(
                "open existing marker directory: {error}"
            )));
        }
    };
    Ok(read(&root))
}

fn empty() -> Marker {
    Marker {
        schema_version: 0,
        managed_by: String::new(),
        target: String::new(),
        name: String::new(),
        rendered_at: String::new(),
        mode: String::new(),
        installed: String::new(),
        source_hash: String::new(),
        allow_executable: false,
        extra: std::collections::BTreeMap::default(),
    }
}

fn decode(bytes: &[u8]) -> Observation {
    let mut result = Observation {
        marker: empty(),
        error: None,
    };
    let fields = match fields(bytes) {
        Ok(fields) => fields,
        Err(error) => {
            result.error = Some(error);
            return result;
        }
    };
    let bytes = bytes.trim_ascii();
    if bytes == b"null" {
        return result;
    }
    if bytes.first() != Some(&b'{') {
        result.error = Some(format!(
            "json: cannot unmarshal {} into Go value of type install.Marker",
            value_type(bytes)
        ));
        return result;
    }
    for (key, raw) in fields {
        let Ok(key) = serde_json::from_str::<String>(&repair_json_strings(key)) else {
            continue;
        };
        let key: String = key
            .chars()
            .map(|ch| match ch {
                '\u{212a}' => 'k',
                '\u{017f}' => 's',
                _ => ch.to_ascii_lowercase(),
            })
            .collect();
        let raw = raw.trim_ascii();
        if raw == b"null" {
            continue;
        }
        let string = match key.as_str() {
            "rendered_at" => Some(&mut result.marker.rendered_at),
            "managed_by" => Some(&mut result.marker.managed_by),
            "target" => Some(&mut result.marker.target),
            "name" => Some(&mut result.marker.name),

            "mode" => Some(&mut result.marker.mode),
            "installed" => Some(&mut result.marker.installed),
            "source_hash" => Some(&mut result.marker.source_hash),
            _ => None,
        };
        let ty = if let Some(field) = string {
            if let Ok(value) = serde_json::from_str::<String>(&repair_json_strings(raw)) {
                *field = value;
                continue;
            }
            match key.as_str() {
                "target" => "render.Target",
                "mode" => "install.Mode",
                _ => "string",
            }
        } else if key == "allow_executable" {
            if let Ok(value) = serde_json::from_slice::<bool>(raw) {
                result.marker.allow_executable = value;
                continue;
            }
            "bool"
        } else if key == "schema_version" {
            // Observation does not use the schema as an ownership selector.
            // Go's field is int, so negative/future versions remain readable.
            if serde_json::from_slice::<i64>(raw).is_ok() {
                continue;
            }
            "int"
        } else {
            continue;
        };
        if result.error.is_none() {
            let value = if value_type(raw) == "number" && ty == "int" {
                format!("number {}", String::from_utf8_lossy(raw))
            } else {
                value_type(raw).into()
            };
            result.error = Some(format!(
                "json: cannot unmarshal {value} into Go struct field Marker.{key} of type {ty}"
            ));
        }
    }
    result
}

fn value_type(raw: &[u8]) -> &'static str {
    match raw.first() {
        Some(b'"') => "string",
        Some(b'{') => "object",
        Some(b'[') => "array",
        Some(b't' | b'f') => "bool",
        Some(b'n') => "null",
        _ => "number",
    }
}
