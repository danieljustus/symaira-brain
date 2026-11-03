//! The wording of Go's `encoding/json`, `time.Parse` and `encoding/base64`
//! errors for the request shapes this server decodes. The shipped server logs
//! these texts verbatim (`ERROR HTTP error … err=…`), so a rejected request
//! reproduces them. The decoders in `sync_decode` own *whether* a body is
//! accepted; this module only explains a rejection.

use std::fmt::Write as _;

/// A decoded JSON value that keeps object member order, which decides which
/// of several type errors Go reports first.
#[derive(Debug, Clone)]
enum Node {
    Null,
    Bool,
    Number(String),
    Text(String),
    Array(Vec<Node>),
    Object(Vec<(String, Node)>),
}

/// Go-side target types.
#[derive(Clone, Copy)]
pub(super) enum Ty {
    Str,
    Int,
    Int64,
    F64,
    F32,
    Bool,
    Time,
    Bytes,
    MapStr,
    StrArr,
    F32Arr,
    /// A struct (or pointer to struct) type.
    Struct(&'static StructTy),
    /// A slice of structs: Go type name of the slice, element.
    Slice(&'static str, &'static StructTy),
    /// Decoded but never inspected.
    Any,
}

/// A Go struct and its JSON members.
pub(super) struct StructTy {
    name: &'static str,
    fields: &'static [(&'static str, Ty)],
}

const MEMORY: StructTy = StructTy {
    name: "db.Memory",
    fields: &[
        ("id", Ty::Str),
        ("content", Ty::Str),
        ("scope", Ty::Str),
        ("metadata", Ty::MapStr),
        ("embedding", Ty::F32Arr),
        ("embedding_source", Ty::Str),
        ("embedding_model", Ty::Str),
        ("embedding_quantization", Ty::Str),
        ("content_hash", Ty::Str),
        ("created_at", Ty::Time),
        ("updated_at", Ty::Time),
        ("created_by", Ty::Str),
        ("updated_by", Ty::Str),
        ("created_session", Ty::Str),
        ("updated_session", Ty::Str),
        ("entities", Ty::StrArr),
        ("consolidation_status", Ty::Str),
        ("consolidated_into_id", Ty::Str),
        ("importance", Ty::F64),
        ("valid_from", Ty::Time),
        ("valid_to", Ty::Time),
        ("superseded_by", Ty::Str),
        ("kind", Ty::Str),
        ("review_status", Ty::Str),
        ("decay_factor", Ty::F64),
        ("retired_at", Ty::Time),
        ("tier", Ty::Str),
        ("expires_at", Ty::Time),
        ("access_count", Ty::Int64),
        ("last_access", Ty::Time),
        ("prev_access", Ty::Time),
        ("evidence", Ty::Any),
    ],
};

const DELETED: StructTy = StructTy {
    name: "db.DeletedMemory",
    fields: &[("id", Ty::Str), ("deleted_at", Ty::Time)],
};

const RELAY_BLOB: StructTy = StructTy {
    name: "db.RelayBlob",
    fields: &[
        ("id", Ty::Str),
        ("updated_at", Ty::Time),
        ("blob", Ty::Bytes),
    ],
};

/// The anonymous request structs, with the Go type text used when the whole
/// body has the wrong JSON kind.
pub(super) enum Shape {
    Set,
    Search,
    Revoke,
    Apply,
    Relay,
}

const SET: StructTy = StructTy {
    name: "struct { Content string \"json:\\\"content\\\"\"; Scope string \"json:\\\"scope\\\"\"; Metadata map[string]string \"json:\\\"metadata\\\"\"; SessionID string \"json:\\\"session_id\\\"\"; Entities []string \"json:\\\"entities\\\"\"; Working bool \"json:\\\"working\\\"\" }",
    fields: &[
        ("content", Ty::Str),
        ("scope", Ty::Str),
        ("metadata", Ty::MapStr),
        ("session_id", Ty::Str),
        ("entities", Ty::StrArr),
        ("working", Ty::Bool),
    ],
};

const SEARCH: StructTy = StructTy {
    name: "struct { Query string \"json:\\\"query\\\"\"; Scope string \"json:\\\"scope\\\"\"; Limit int \"json:\\\"limit\\\"\"; Entity string \"json:\\\"entity\\\"\"; MinConfidence string \"json:\\\"min_confidence\\\"\"; Verification string \"json:\\\"verification\\\"\"; ExcludeSuperseded bool \"json:\\\"exclude_superseded\\\"\"; MaxAge string \"json:\\\"max_age\\\"\"; MaxSensitivity string \"json:\\\"max_sensitivity\\\"\"; MinSharingLevel string \"json:\\\"min_sharing_level\\\"\"; ClientID string \"json:\\\"client_id\\\"\" }",
    fields: &[
        ("query", Ty::Str),
        ("scope", Ty::Str),
        ("limit", Ty::Int),
        ("entity", Ty::Str),
        ("min_confidence", Ty::Str),
        ("verification", Ty::Str),
        ("exclude_superseded", Ty::Bool),
        ("max_age", Ty::Str),
        ("max_sensitivity", Ty::Str),
        ("min_sharing_level", Ty::Str),
        ("client_id", Ty::Str),
    ],
};

const REVOKE: StructTy = StructTy {
    name: "struct { Token string \"json:\\\"token\\\"\"; JTI string \"json:\\\"jti\\\"\" }",
    fields: &[("token", Ty::Str), ("jti", Ty::Str)],
};

const APPLY: StructTy = StructTy {
    name: "struct { Memories []*db.Memory \"json:\\\"memories\\\"\"; Deleted []db.DeletedMemory \"json:\\\"deleted\\\"\" }",
    fields: &[
        ("memories", Ty::Slice("[]*db.Memory", &MEMORY)),
        ("deleted", Ty::Slice("[]db.DeletedMemory", &DELETED)),
    ],
};

const RELAY: StructTy = StructTy {
    name: "struct { Blobs []db.RelayBlob \"json:\\\"blobs\\\"\" }",
    fields: &[("blobs", Ty::Slice("[]db.RelayBlob", &RELAY_BLOB))],
};

impl Shape {
    fn ty(&self) -> &'static StructTy {
        match self {
            Self::Set => &SET,
            Self::Search => &SEARCH,
            Self::Revoke => &REVOKE,
            Self::Apply => &APPLY,
            Self::Relay => &RELAY,
        }
    }
}

/// `json.Unmarshal(body, &shape)` error text, for a body the caller already
/// knows is rejected.
pub(super) fn unmarshal_error(body: &[u8], shape: &Shape) -> String {
    if let Some(message) = syntax_error(body) {
        return message;
    }
    let mut parser = Parser { bytes: body, at: 0 };
    parser.space();
    let root = parser.value();
    let Some(root) = root else {
        return "unexpected end of JSON input".to_owned();
    };
    let mut walk = Walk::default();
    let target = shape.ty();
    match &root {
        Node::Object(_) => {
            walk.object(&root, target, "");
            walk.saved
                .unwrap_or_else(|| "json: invalid request".to_owned())
        }
        Node::Null => "json: invalid request".to_owned(),
        other => format!(
            "json: cannot unmarshal {} into Go value of type {}",
            kind(other, None),
            target.name
        ),
    }
}

#[derive(Default)]
struct Walk {
    saved: Option<String>,
}

impl Walk {
    fn save(&mut self, message: String) {
        if self.saved.is_none() {
            self.saved = Some(message);
        }
    }

    fn mismatch(&mut self, node: &Node, ty: &str, path: &str, int_literal: bool) {
        let value = kind(node, int_literal.then_some(()));
        let message = if path.rsplit('.').next().is_some_and(is_index) {
            format!("json: cannot unmarshal {value} into {path} of type {ty}")
        } else {
            format!("json: cannot unmarshal {value} into Go struct field {path} of type {ty}")
        };
        self.save(message);
    }

    /// Mirrors `decode.object` for a struct target; the first error in
    /// document order wins, including `UnmarshalJSON` (time) failures.
    fn object(&mut self, node: &Node, target: &StructTy, path: &str) {
        let Node::Object(members) = node else {
            return;
        };
        for (key, value) in members {
            let field = target
                .fields
                .iter()
                .find(|(name, _)| name == key)
                .or_else(|| {
                    target
                        .fields
                        .iter()
                        .find(|(name, _)| name.eq_ignore_ascii_case(key))
                });
            if let Some((name, ty)) = field {
                self.value(value, *ty, &format!("{path}.{name}"));
            }
        }
    }

    fn value(&mut self, node: &Node, ty: Ty, path: &str) {
        if matches!(node, Node::Null) {
            return;
        }
        match ty {
            Ty::Any => {}
            Ty::Str => {
                if !matches!(node, Node::Text(_)) {
                    self.mismatch(node, "string", path, false);
                }
            }
            Ty::Bool => {
                if !matches!(node, Node::Bool) {
                    self.mismatch(node, "bool", path, false);
                }
            }
            Ty::Int | Ty::Int64 => {
                let name = if matches!(ty, Ty::Int) {
                    "int"
                } else {
                    "int64"
                };
                match node {
                    Node::Number(text) if text.parse::<i64>().is_ok() => {}
                    Node::Number(_) => self.mismatch(node, name, path, true),
                    _ => self.mismatch(node, name, path, false),
                }
            }
            Ty::F64 | Ty::F32 => {
                let name = if matches!(ty, Ty::F64) {
                    "float64"
                } else {
                    "float32"
                };
                if !matches!(node, Node::Number(_)) {
                    self.mismatch(node, name, path, false);
                }
            }
            Ty::Time => match node {
                Node::Text(text) => {
                    if let Some(detail) = time_error(text, RFC3339) {
                        self.save(detail);
                    }
                }
                _ => self.mismatch(node, "time.Time", path, false),
            },
            Ty::Bytes => match node {
                Node::Text(text) => {
                    if let Some(position) = base64_error(text, false) {
                        self.save(format!(
                            "json: cannot unmarshal string into Go struct field {path} of type []uint8: illegal base64 data at input byte {position}"
                        ));
                    }
                }
                Node::Array(_) => {}
                _ => self.mismatch(node, "[]uint8", path, false),
            },
            Ty::MapStr => match node {
                Node::Object(members) => {
                    for (key, value) in members {
                        self.value(value, Ty::Str, &format!("{path}.{key}"));
                    }
                }
                _ => self.mismatch(node, "map[string]string", path, false),
            },
            Ty::StrArr => match node {
                Node::Array(items) => {
                    for (index, item) in items.iter().enumerate() {
                        self.value(item, Ty::Str, &format!("{path}.{index}"));
                    }
                }
                _ => self.mismatch(node, "[]string", path, false),
            },
            Ty::F32Arr => match node {
                Node::Array(items) => {
                    for (index, item) in items.iter().enumerate() {
                        self.value(item, Ty::F32, &format!("{path}.{index}"));
                    }
                }
                _ => self.mismatch(node, "[]float32", path, false),
            },
            Ty::Struct(target) => match node {
                Node::Object(_) => self.object(node, target, path),
                _ => self.mismatch(node, target.name, path, false),
            },
            Ty::Slice(slice, element) => match node {
                Node::Array(items) => {
                    for (index, item) in items.iter().enumerate() {
                        self.value(item, Ty::Struct(element), &format!("{path}.{index}"));
                    }
                }
                _ => self.mismatch(node, slice, path, false),
            },
        }
    }
}

fn is_index(segment: &str) -> bool {
    !segment.is_empty() && segment.bytes().all(|byte| byte.is_ascii_digit())
}

fn kind(node: &Node, literal: Option<()>) -> String {
    match node {
        Node::Null => "null".into(),
        Node::Bool => "bool".into(),
        Node::Number(text) if literal.is_some() => format!("number {text}"),
        Node::Number(_) => "number".into(),
        Node::Text(_) => "string".into(),
        Node::Array(_) => "array".into(),
        Node::Object(_) => "object".into(),
    }
}

// ----------------------------------------------------------------- parsing

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        while matches!(self.bytes.get(self.at), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.at += 1;
        }
    }

    /// Parses a syntactically valid value (validation happened before).
    fn value(&mut self) -> Option<Node> {
        self.space();
        match *self.bytes.get(self.at)? {
            b'{' => {
                self.at += 1;
                let mut members = Vec::new();
                loop {
                    self.space();
                    match *self.bytes.get(self.at)? {
                        b'}' => {
                            self.at += 1;
                            return Some(Node::Object(members));
                        }
                        b',' => self.at += 1,
                        _ => {
                            let key = self.string()?;
                            self.space();
                            self.at += 1; // ':'
                            members.push((key, self.value()?));
                        }
                    }
                }
            }
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                loop {
                    self.space();
                    match *self.bytes.get(self.at)? {
                        b']' => {
                            self.at += 1;
                            return Some(Node::Array(items));
                        }
                        b',' => self.at += 1,
                        _ => items.push(self.value()?),
                    }
                }
            }
            b'"' => self.string().map(Node::Text),
            b't' => {
                self.at += 4;
                Some(Node::Bool)
            }
            b'f' => {
                self.at += 5;
                Some(Node::Bool)
            }
            b'n' => {
                self.at += 4;
                Some(Node::Null)
            }
            _ => {
                let start = self.at;
                while self.bytes.get(self.at).is_some_and(|byte| {
                    matches!(byte, b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
                }) {
                    self.at += 1;
                }
                Some(Node::Number(
                    String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned(),
                ))
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        let start = self.at;
        self.at += 1;
        while *self.bytes.get(self.at)? != b'"' {
            self.at += if self.bytes[self.at] == b'\\' { 2 } else { 1 };
        }
        self.at += 1;
        serde_json::from_slice::<String>(&self.bytes[start..self.at]).ok()
    }
}

// ------------------------------------------------------- syntax validation

/// Go `checkValid`: the first syntax error of the document, if any.
pub(super) fn syntax_error(input: &[u8]) -> Option<String> {
    let mut check = Check { input, at: 0 };
    check.document().err()
}

struct Check<'a> {
    input: &'a [u8],
    at: usize,
}

const EOF: &str = "unexpected end of JSON input";

impl Check<'_> {
    fn peek(&self) -> Option<u8> {
        self.input.get(self.at).copied()
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.at += 1;
        }
    }

    fn bad(&self, context: &str) -> String {
        match self.peek() {
            None => EOF.to_owned(),
            Some(byte) => format!(
                "invalid character {} {context}",
                quote_char(&self.input[self.at..], byte)
            ),
        }
    }

    fn document(&mut self) -> Result<(), String> {
        self.space();
        self.value()?;
        self.space();
        if self.peek().is_some() {
            return Err(self.bad("after top-level value"));
        }
        Ok(())
    }

    fn value(&mut self) -> Result<(), String> {
        match self.peek() {
            None => Err(EOF.to_owned()),
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

    fn literal(&mut self, word: &str) -> Result<(), String> {
        for (index, expected) in word.bytes().enumerate() {
            if index == 0 {
                self.at += 1;
                continue;
            }
            match self.peek() {
                None => return Err(EOF.to_owned()),
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

    fn object(&mut self) -> Result<(), String> {
        self.at += 1;
        self.space();
        if self.peek() == Some(b'}') {
            self.at += 1;
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
                    return Ok(());
                }
                _ => return Err(self.bad("after object key:value pair")),
            }
        }
    }

    fn array(&mut self) -> Result<(), String> {
        self.at += 1;
        self.space();
        if self.peek() == Some(b']') {
            self.at += 1;
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
                    return Ok(());
                }
                _ => return Err(self.bad("after array element")),
            }
        }
    }

    fn string(&mut self) -> Result<(), String> {
        self.at += 1;
        loop {
            match self.peek() {
                None => return Err(EOF.to_owned()),
                Some(b'"') => {
                    self.at += 1;
                    return Ok(());
                }
                Some(b'\\') => {
                    let backslash = self.at;
                    self.at += 1;
                    match self.peek() {
                        None => return Err(EOF.to_owned()),
                        Some(b'b' | b'f' | b'n' | b'r' | b't' | b'\\' | b'/' | b'"') => {
                            self.at += 1;
                        }
                        Some(b'u') => {
                            self.at += 1;
                            for _ in 0..4 {
                                match self.peek() {
                                    None => return Err(EOF.to_owned()),
                                    Some(byte) if byte.is_ascii_hexdigit() => self.at += 1,
                                    Some(_) => {
                                        // The message quotes `\u` and the next four
                                        // input bytes (fewer at the end).
                                        let from = backslash + 2;
                                        let to = (from + 4).min(self.input.len());
                                        return Err(format!(
                                            "invalid escape sequence `\\u{}` in string",
                                            String::from_utf8_lossy(&self.input[from..to])
                                        ));
                                    }
                                }
                            }
                        }
                        Some(byte) => {
                            return Err(format!(
                                "invalid escape sequence `\\{}` in string",
                                char::from(byte)
                            ));
                        }
                    }
                }
                Some(byte) if byte < 0x20 => return Err(self.bad("in string")),
                Some(_) => self.at += 1,
            }
        }
    }

    fn digits(&mut self, context: &str) -> Result<(), String> {
        match self.peek() {
            Some(b'0'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.at += 1;
                }
                Ok(())
            }
            _ => Err(self.bad(context)),
        }
    }

    fn number(&mut self) -> Result<(), String> {
        if self.peek() == Some(b'-') {
            self.at += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.bad("in numeric literal"));
            }
        }
        if self.peek() == Some(b'0') {
            self.at += 1;
        } else {
            self.digits("in numeric literal")?;
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            self.digits("in numeric literal")?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            self.digits("in numeric literal")?;
        }
        Ok(())
    }
}

/// Go `quoteChar`.
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

/// `strconv.Quote` for the characters that can appear in these messages.
pub(super) fn go_quote(text: &str) -> String {
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
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// -------------------------------------------------------------------- time

/// `time.RFC3339`.
pub(super) const RFC3339: &str = "2006-01-02T15:04:05Z07:00";
/// `time.RFC3339Nano`.
pub(super) const RFC3339_NANO: &str = "2006-01-02T15:04:05.999999999Z07:00";

/// The `time.Parse` error for an RFC 3339 `value`, or `None` when it parses.
/// Covers the failing element of the layout (`cannot parse … as …`) and the
/// range errors.
pub(super) fn time_error(value: &str, layout: &str) -> Option<String> {
    if super::rfc3339::parse(value).is_some() {
        return None;
    }
    let bytes = value.as_bytes();
    let mut at = 0_usize;
    let fail = |at: usize, element: &str| {
        Some(format!(
            "parsing time {} as {}: cannot parse {} as {}",
            go_quote(value),
            go_quote(layout),
            go_quote(&value[at.min(value.len())..]),
            go_quote(element)
        ))
    };
    let range = |what: &str| {
        Some(format!(
            "parsing time {}: {what} out of range",
            go_quote(value)
        ))
    };
    let digits = |at: usize, count: usize| -> Option<u32> {
        let slice = bytes.get(at..at + count)?;
        slice
            .iter()
            .all(u8::is_ascii_digit)
            .then(|| std::str::from_utf8(slice).ok()?.parse().ok())
            .flatten()
    };
    let Some(year) = digits(at, 4) else {
        return fail(at, "2006");
    };
    let _ = year;
    at += 4;
    for (literal, element, count) in [
        ('-', "01", 2_usize),
        ('-', "02", 2),
        ('T', "15", 2),
        (':', "04", 2),
        (':', "05", 2),
    ] {
        let lit = format!("{literal}");
        if bytes.get(at).copied() != Some(literal as u8) {
            return fail(at, &lit);
        }
        at += 1;
        let Some(number) = digits(at, count) else {
            return fail(at, element);
        };
        let limit = match element {
            "01" => (1, 12, "month"),
            "02" => (1, 31, "day"),
            "15" => (0, 23, "hour"),
            "04" => (0, 59, "minute"),
            _ => (0, 59, "second"),
        };
        if number < limit.0 || number > limit.1 {
            return range(limit.2);
        }
        at += count;
    }
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        let start = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == start {
            return fail(at - 1, "Z07:00");
        }
    }
    match bytes.get(at) {
        Some(b'Z') if at + 1 == bytes.len() => None,
        Some(b'+' | b'-') => {
            let ok = bytes.len() == at + 6
                && digits(at + 1, 2).is_some()
                && bytes[at + 3] == b':'
                && digits(at + 4, 2).is_some();
            if ok {
                range("time zone offset hour")
            } else {
                fail(at, "Z07:00")
            }
        }
        Some(b'Z') => Some(format!(
            "parsing time {}: extra text: {}",
            go_quote(value),
            go_quote(&value[at + 1..])
        )),
        _ => fail(at, "Z07:00"),
    }
}

// ------------------------------------------------------------------ base64

/// `base64.StdEncoding.DecodeString` failure position (`CorruptInputError`).
pub(super) fn base64_error(text: &str, url: bool) -> Option<usize> {
    let src = text.as_bytes();
    let (plus, slash) = if url { (b'-', b'_') } else { (b'+', b'/') };
    let valid = |byte: u8| byte.is_ascii_alphanumeric() || byte == plus || byte == slash;
    let mut si = 0_usize;
    while si < src.len() {
        let mut j = 0_usize;
        while j < 4 {
            if si == src.len() {
                if j == 0 {
                    return None;
                }
                return Some(si - j);
            }
            let byte = src[si];
            si += 1;
            if valid(byte) {
                j += 1;
                continue;
            }
            if byte == b'\n' || byte == b'\r' {
                continue;
            }
            if byte != b'=' {
                return Some(si - 1);
            }
            match j {
                0 | 1 => return Some(si - 1),
                2 => {
                    if si == src.len() {
                        return Some(src.len());
                    }
                    if src[si] != b'=' {
                        return Some(si - 1);
                    }
                    si += 1;
                }
                _ => {}
            }
            while si < src.len() && (src[si] == b'\n' || src[si] == b'\r') {
                si += 1;
            }
            if si < src.len() {
                return Some(si);
            }
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(body: &str) -> String {
        unmarshal_error(body.as_bytes(), &Shape::Set)
    }

    #[test]
    fn syntax_errors_use_the_shipped_wording() {
        assert_eq!(set("{"), "unexpected end of JSON input");
        assert_eq!(set(""), "unexpected end of JSON input");
        assert_eq!(
            set("{\"a\":1,}"),
            "invalid character '}' looking for beginning of object key string"
        );
        assert_eq!(set("{\"a\" 1}"), "invalid character '1' after object key");
        assert_eq!(
            set("{\"a\":1 \"b\"}"),
            "invalid character '\"' after object key:value pair"
        );
        assert_eq!(set("{} {}"), "invalid character '{' after top-level value");
        assert_eq!(set("nul"), "unexpected end of JSON input");
        assert_eq!(
            set("nulx"),
            "invalid character 'x' in literal null (expecting 'l')"
        );
        assert_eq!(set("[1 2]"), "invalid character '2' after array element");
        assert_eq!(set("-x"), "invalid character 'x' in numeric literal");
        assert_eq!(
            set(r#"{"a":"\u12G4"}"#),
            "invalid escape sequence `\\u12G4` in string"
        );
        assert_eq!(
            set(r#"{"a":"\u"}"#),
            "invalid escape sequence `\\u\"}` in string"
        );
        assert_eq!(set("1.x"), "invalid character 'x' in numeric literal");
    }

    #[test]
    fn type_errors_name_the_path_and_go_type() {
        assert_eq!(
            set(r#"{"content":1}"#),
            "json: cannot unmarshal number into Go struct field .content of type string"
        );
        assert_eq!(
            set(r#"{"metadata":{"a":1}}"#),
            "json: cannot unmarshal number into Go struct field .metadata.a of type string"
        );
        assert_eq!(
            set(r#"{"entities":[1]}"#),
            "json: cannot unmarshal number into .entities.0 of type string"
        );
        assert!(set("[]").starts_with(
            "json: cannot unmarshal array into Go value of type struct { Content string"
        ));
        assert_eq!(
            unmarshal_error(br#"{"limit":1.5}"#, &Shape::Search),
            "json: cannot unmarshal number 1.5 into Go struct field .limit of type int"
        );
        assert_eq!(
            unmarshal_error(br#"{"memories":[{"created_at":"bad"}]}"#, &Shape::Apply),
            "parsing time \"bad\" as \"2006-01-02T15:04:05Z07:00\": cannot parse \"bad\" as \"2006\""
        );
        assert_eq!(
            unmarshal_error(br#"{"blobs":[{"blob":"***"}]}"#, &Shape::Relay),
            "json: cannot unmarshal string into Go struct field .blobs.0.blob of type []uint8: illegal base64 data at input byte 0"
        );
    }

    #[test]
    fn base64_positions_follow_the_standard_decoder() {
        assert_eq!(base64_error("aGVsbG8=", false), None);
        assert_eq!(base64_error("***", false), Some(0));
        assert_eq!(base64_error("aGVsbG8", false), Some(4));
        assert_eq!(base64_error("aG==", false), None);
        assert_eq!(base64_error("aG=", false), Some(3));
    }

    #[test]
    fn time_errors_cover_literals_and_ranges() {
        assert_eq!(time_error("2030-01-02T03:04:05Z", RFC3339), None);
        assert_eq!(
            time_error("2030-13-02T03:04:05Z", RFC3339).unwrap(),
            "parsing time \"2030-13-02T03:04:05Z\": month out of range"
        );
        assert_eq!(
            time_error("2030-01-02 03:04:05Z", RFC3339).unwrap(),
            "parsing time \"2030-01-02 03:04:05Z\" as \"2006-01-02T15:04:05Z07:00\": cannot parse \" 03:04:05Z\" as \"T\""
        );
    }
}
