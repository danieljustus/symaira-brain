//! `encoding/json` as the shipped sync client uses it: decoding of the relay's
//! replies into the `db.Memory` / `db.DeletedMemory` / `db.RelayBlob` shapes
//! with Go's reflection semantics (case-insensitive keys, last duplicate wins,
//! `null` leaves scalars alone and clears maps and slices, the first type
//! error is reported at the end while the decode continues, an `UnmarshalJSON`
//! failure aborts at once) and the `json.Marshal` renderings of the push
//! bodies.

use crate::sync_client_scan::{Fail, check_valid, first_value, go_lossy, unquote};
use crate::sync_client_time::{GoTime, unmarshal_json as time_from_json};
use std::collections::BTreeMap;

// ------------------------------------------------------------------ nodes

/// A syntactically valid JSON value that keeps member order and duplicates.
#[derive(Debug)]
enum Node {
    Null,
    Bool,
    Number(String),
    Str { value: String, raw: Vec<u8> },
    Array(Vec<Node>),
    Object(Vec<(String, Node)>),
}

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

    fn token_string(&mut self) -> (String, Vec<u8>) {
        let start = self.at;
        self.at += 1;
        while self.bytes[self.at] != b'"' {
            self.at += if self.bytes[self.at] == b'\\' { 2 } else { 1 };
        }
        self.at += 1;
        let token = &self.bytes[start..self.at];
        (unquote(token), token.to_vec())
    }

    /// Parses a value the scanner already accepted.
    fn value(&mut self) -> Node {
        self.space();
        match self.bytes[self.at] {
            b'{' => {
                self.at += 1;
                let mut members = Vec::new();
                loop {
                    self.space();
                    match self.bytes[self.at] {
                        b'}' => {
                            self.at += 1;
                            return Node::Object(members);
                        }
                        b',' => self.at += 1,
                        _ => {
                            let (key, _) = self.token_string();
                            self.space();
                            self.at += 1;
                            members.push((key, self.value()));
                        }
                    }
                }
            }
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                loop {
                    self.space();
                    match self.bytes[self.at] {
                        b']' => {
                            self.at += 1;
                            return Node::Array(items);
                        }
                        b',' => self.at += 1,
                        _ => items.push(self.value()),
                    }
                }
            }
            b'"' => {
                let (value, raw) = self.token_string();
                Node::Str { value, raw }
            }
            b't' => {
                self.at += 4;
                Node::Bool
            }
            b'f' => {
                self.at += 5;
                Node::Bool
            }
            b'n' => {
                self.at += 4;
                Node::Null
            }
            _ => {
                let start = self.at;
                while self.bytes.get(self.at).is_some_and(|byte| {
                    matches!(byte, b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
                }) {
                    self.at += 1;
                }
                Node::Number(String::from_utf8_lossy(&self.bytes[start..self.at]).into_owned())
            }
        }
    }
}

// ------------------------------------------------------------ type schema

/// A Go field type.
#[derive(Clone, Copy)]
pub(crate) enum Ty {
    Str,
    Int,
    I64,
    F64,
    F32,
    U8,
    Time,
    TimePtr,
    Bytes,
    MapStr,
    Slice(&'static Ty),
    Ptr(&'static Ty),
    Struct(&'static StructTy),
}

/// A Go struct and its JSON members (the exported, untagged-`-` ones).
pub(crate) struct StructTy {
    name: &'static str,
    full: &'static str,
    fields: &'static [(&'static str, Ty)],
}

const STRING: Ty = Ty::Str;
const FLOAT32: Ty = Ty::F32;
const UINT8: Ty = Ty::U8;

static EVIDENCE: StructTy = StructTy {
    name: "EvidenceSpan",
    full: "db.EvidenceSpan",
    fields: &[
        ("id", Ty::Str),
        ("memory_id", Ty::Str),
        ("source_id", Ty::Str),
        ("source_kind", Ty::Str),
        ("text", Ty::Str),
        ("evidence_text", Ty::Str),
        ("char_start", Ty::Int),
        ("char_end", Ty::Int),
        ("alignment_status", Ty::Str),
        ("created_at", Ty::Time),
    ],
};

static EVIDENCE_TY: Ty = Ty::Struct(&EVIDENCE);

static MEMORY: StructTy = StructTy {
    name: "Memory",
    full: "db.Memory",
    fields: &[
        ("id", Ty::Str),
        ("content", Ty::Str),
        ("scope", Ty::Str),
        ("metadata", Ty::MapStr),
        ("embedding", Ty::Slice(&FLOAT32)),
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
        ("entities", Ty::Slice(&STRING)),
        ("consolidation_status", Ty::Str),
        ("consolidated_into_id", Ty::Str),
        ("importance", Ty::F64),
        ("valid_from", Ty::TimePtr),
        ("valid_to", Ty::TimePtr),
        ("superseded_by", Ty::Str),
        ("kind", Ty::Str),
        ("review_status", Ty::Str),
        ("decay_factor", Ty::F64),
        ("retired_at", Ty::TimePtr),
        ("tier", Ty::Str),
        ("expires_at", Ty::TimePtr),
        ("access_count", Ty::I64),
        ("last_access", Ty::TimePtr),
        ("prev_access", Ty::TimePtr),
        ("evidence", Ty::Slice(&EVIDENCE_TY)),
    ],
};

static MEMORY_TY: Ty = Ty::Struct(&MEMORY);
static MEMORY_PTR: Ty = Ty::Ptr(&MEMORY_TY);

static DELETED: StructTy = StructTy {
    name: "DeletedMemory",
    full: "db.DeletedMemory",
    fields: &[("id", Ty::Str), ("deleted_at", Ty::Time)],
};
static DELETED_TY: Ty = Ty::Struct(&DELETED);

static CHANGES: StructTy = StructTy {
    name: "ChangesResponse",
    full: "syncclient.ChangesResponse",
    fields: &[
        ("memories", Ty::Slice(&MEMORY_PTR)),
        ("deleted", Ty::Slice(&DELETED_TY)),
        ("server_time", Ty::Time),
        ("next_cursor", Ty::Str),
    ],
};

static RELAY_BLOB: StructTy = StructTy {
    name: "RelayBlob",
    full: "db.RelayBlob",
    fields: &[
        ("id", Ty::Str),
        ("updated_at", Ty::Time),
        ("blob", Ty::Bytes),
    ],
};
static RELAY_BLOB_TY: Ty = Ty::Struct(&RELAY_BLOB);

static RELAY_RESPONSE: StructTy = StructTy {
    name: "RelayResponse",
    full: "syncclient.RelayResponse",
    fields: &[
        ("blobs", Ty::Slice(&RELAY_BLOB_TY)),
        ("server_time", Ty::Time),
    ],
};

static RELAY_PUSH_RESULT: StructTy = StructTy {
    name: "RelayPushResult",
    full: "syncclient.RelayPushResult",
    fields: &[("stored", Ty::Int), ("skipped", Ty::Int)],
};

static RELAY_PAYLOAD: StructTy = StructTy {
    name: "relayPayload",
    full: "syncclient.relayPayload",
    fields: &[
        ("memory", Ty::Ptr(&MEMORY_TY)),
        ("deleted", Ty::Ptr(&DELETED_TY)),
    ],
};

static API_ERROR: StructTy = StructTy {
    name: "",
    full: "struct { Error string; Code string }",
    fields: &[("error", Ty::Str), ("code", Ty::Str)],
};

static COUNT_ERROR: StructTy = StructTy {
    name: "",
    full: "int",
    fields: &[],
};

fn type_name(ty: Ty) -> String {
    match ty {
        Ty::Str => "string".to_owned(),
        Ty::Int => "int".to_owned(),
        Ty::I64 => "int64".to_owned(),
        Ty::F64 => "float64".to_owned(),
        Ty::F32 => "float32".to_owned(),
        Ty::U8 => "uint8".to_owned(),
        Ty::Time => "time.Time".to_owned(),
        Ty::TimePtr => "*time.Time".to_owned(),
        Ty::Bytes => "[]uint8".to_owned(),
        Ty::MapStr => "map[string]string".to_owned(),
        Ty::Slice(element) => format!("[]{}", type_name(*element)),
        Ty::Ptr(target) => format!("*{}", type_name(*target)),
        Ty::Struct(shape) => shape.full.to_owned(),
    }
}

// ----------------------------------------------------------------- values

/// A Go value being filled by the decoder.
#[derive(Clone, Debug)]
pub(crate) enum Val {
    Str(String),
    Int(i64),
    F64(f64),
    F32(f32),
    U8(u8),
    Time(GoTime),
    TimePtr(Option<GoTime>),
    Bytes(Option<Vec<u8>>),
    Map(Option<BTreeMap<String, String>>),
    Slice(Option<Vec<Val>>),
    Ptr(Option<Box<Val>>),
    Struct(Vec<Val>),
}

fn zero(ty: Ty) -> Val {
    match ty {
        Ty::Str => Val::Str(String::new()),
        Ty::Int | Ty::I64 => Val::Int(0),
        Ty::F64 => Val::F64(0.0),
        Ty::F32 => Val::F32(0.0),
        Ty::U8 => Val::U8(0),
        Ty::Time => Val::Time(GoTime::zero()),
        Ty::TimePtr => Val::TimePtr(None),
        Ty::Bytes => Val::Bytes(None),
        Ty::MapStr => Val::Map(None),
        Ty::Slice(_) => Val::Slice(None),
        Ty::Ptr(_) => Val::Ptr(None),
        Ty::Struct(shape) => Val::Struct(shape.fields.iter().map(|(_, ty)| zero(*ty)).collect()),
    }
}

/// Go's `foldName` for the ASCII member names used here.
fn fold(key: &str) -> String {
    key.chars()
        .map(|c| match c {
            '\u{17f}' => 's',
            '\u{212a}' => 'k',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

/// A failure that ends the decode at once (an `UnmarshalJSON` error).
type Abort = String;

struct Decoder {
    saved: Option<String>,
    structure: Option<&'static str>,
    /// The JSON path below the root struct: member names, array indices and
    /// map keys, as `encoding/json` names the field of a type error.
    fields: Vec<String>,
}

impl Decoder {
    fn new() -> Self {
        Self {
            saved: None,
            structure: None,
            fields: Vec::new(),
        }
    }

    fn mismatch(&mut self, found: &str, ty: Ty) {
        self.mismatch_because(found, ty, None);
    }

    fn mismatch_because(&mut self, found: &str, ty: Ty, cause: Option<&str>) {
        if self.saved.is_some() {
            return;
        }
        let target = type_name(ty);
        let mut text = if self.structure.is_some() || !self.fields.is_empty() {
            format!(
                "json: cannot unmarshal {found} into Go struct field {}.{} of type {target}",
                self.structure.unwrap_or(""),
                self.fields.join(".")
            )
        } else {
            format!("json: cannot unmarshal {found} into Go value of type {target}")
        };
        if let Some(cause) = cause {
            text.push_str(": ");
            text.push_str(cause);
        }
        self.saved = Some(text);
    }

    fn value(&mut self, node: &Node, ty: Ty, target: &mut Val) -> Result<(), Abort> {
        match node {
            Node::Array(items) => self.array(items, ty, target),
            Node::Object(members) => self.object(members, ty, target),
            _ => self.literal(node, ty, target),
        }
    }

    /// `indirect` for a non-null value: allocates pointers and returns the
    /// innermost type and slot.
    fn deref(ty: Ty, target: &mut Val) -> (Ty, &mut Val) {
        match (ty, target) {
            (Ty::Ptr(inner), Val::Ptr(slot)) => {
                let boxed = slot.get_or_insert_with(|| Box::new(zero(*inner)));
                Self::deref(*inner, boxed)
            }
            (ty, target) => (ty, target),
        }
    }

    fn time_from(node: &Node) -> Result<Option<GoTime>, Abort> {
        match node {
            Node::Null => time_from_json("null"),
            Node::Str { raw, .. } => time_from_json(&String::from_utf8_lossy(raw)),
            _ => time_from_json("0"),
        }
    }

    fn assign_time(ty: Ty, target: &mut Val, node: &Node) -> Result<(), Abort> {
        let parsed = Self::time_from(node)?;
        match (ty, target) {
            (Ty::TimePtr, Val::TimePtr(slot)) => {
                if let Some(time) = parsed {
                    *slot = Some(time);
                }
            }
            (_, Val::Time(slot)) => {
                if let Some(time) = parsed {
                    *slot = time;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn array(&mut self, items: &[Node], ty: Ty, target: &mut Val) -> Result<(), Abort> {
        let (ty, target) = Self::deref(ty, target);
        match (ty, target) {
            (Ty::Time | Ty::TimePtr, slot) => Self::assign_time(ty, slot, &Node::Array(Vec::new())),
            (Ty::Slice(element), Val::Slice(slot)) => {
                let vector = slot.get_or_insert_with(Vec::new);
                for (index, item) in items.iter().enumerate() {
                    if index >= vector.len() {
                        vector.push(zero(*element));
                    }
                    self.fields.push(index.to_string());
                    let outcome = self.value(item, *element, &mut vector[index]);
                    self.fields.pop();
                    outcome?;
                }
                vector.truncate(items.len());
                Ok(())
            }
            (Ty::Bytes, Val::Bytes(slot)) => {
                let vector = slot.get_or_insert_with(Vec::new);
                for (index, item) in items.iter().enumerate() {
                    if index >= vector.len() {
                        vector.push(0);
                    }
                    let mut element = Val::U8(vector[index]);
                    self.value(item, UINT8, &mut element)?;
                    if let Val::U8(byte) = element {
                        vector[index] = byte;
                    }
                }
                vector.truncate(items.len());
                Ok(())
            }
            (other, _) => {
                self.mismatch("array", other);
                Ok(())
            }
        }
    }

    fn object(
        &mut self,
        members: &[(String, Node)],
        ty: Ty,
        target: &mut Val,
    ) -> Result<(), Abort> {
        let (ty, target) = Self::deref(ty, target);
        match (ty, target) {
            (Ty::Time | Ty::TimePtr, slot) => {
                Self::assign_time(ty, slot, &Node::Object(Vec::new()))
            }
            (Ty::MapStr, Val::Map(slot)) => {
                let map = slot.get_or_insert_with(BTreeMap::new);
                for (key, node) in members {
                    let mut element = Val::Str(String::new());
                    self.fields.push(key.clone());
                    let outcome = self.value(node, Ty::Str, &mut element);
                    self.fields.pop();
                    outcome?;
                    if let Val::Str(text) = element {
                        map.insert(key.clone(), text);
                    }
                }
                Ok(())
            }
            (Ty::Struct(shape), Val::Struct(values)) => {
                // The error names the root struct, not the one that holds the field.
                let original = (self.structure, self.fields.len());
                if self.structure.is_none() {
                    self.structure = Some(shape.name);
                }
                for (key, node) in members {
                    let index = shape
                        .fields
                        .iter()
                        .position(|(name, _)| name == key)
                        .or_else(|| {
                            let folded = fold(key);
                            shape.fields.iter().position(|(name, _)| *name == folded)
                        });
                    let Some(index) = index else { continue };
                    let (name, field_ty) = shape.fields[index];
                    self.fields.push(name.to_owned());
                    let outcome = self.value(node, field_ty, &mut values[index]);
                    self.fields.truncate(original.1);
                    if outcome.is_err() {
                        self.structure = original.0;
                        return outcome;
                    }
                }
                self.structure = original.0;
                Ok(())
            }
            (other, _) => {
                self.mismatch("object", other);
                Ok(())
            }
        }
    }

    fn literal(&mut self, node: &Node, ty: Ty, target: &mut Val) -> Result<(), Abort> {
        // A null stops at the first pointer so that it can be cleared.
        if matches!(node, Node::Null) {
            match (ty, &mut *target) {
                (Ty::Ptr(_), Val::Ptr(slot)) => *slot = None,
                (Ty::TimePtr, Val::TimePtr(slot)) => *slot = None,
                (Ty::Slice(_), Val::Slice(slot)) => *slot = None,
                (Ty::Bytes, Val::Bytes(slot)) => *slot = None,
                (Ty::MapStr, Val::Map(slot)) => *slot = None,
                _ => {}
            }
            return Ok(());
        }
        let (ty, target) = Self::deref(ty, target);
        if matches!(ty, Ty::Time | Ty::TimePtr) {
            return Self::assign_time(ty, target, node);
        }
        match node {
            Node::Bool => self.mismatch("bool", ty),
            Node::Str { value, .. } => match (ty, target) {
                (Ty::Str, Val::Str(slot)) => value.clone_into(slot),
                (Ty::Bytes, Val::Bytes(slot)) => match base64_decode(value.as_bytes()) {
                    Ok(bytes) => *slot = Some(bytes),
                    Err(position) => self.mismatch_because(
                        "string",
                        ty,
                        Some(&format!("illegal base64 data at input byte {position}")),
                    ),
                },
                (other, _) => self.mismatch("string", other),
            },
            Node::Number(text) => self.number(text, ty, target),
            // Null was handled above; arrays and objects never reach here.
            _ => {}
        }
        Ok(())
    }

    fn number(&mut self, text: &str, ty: Ty, target: &mut Val) {
        let found = format!("number {text}");
        match (ty, target) {
            (Ty::Int | Ty::I64, Val::Int(slot)) => match text.parse::<i64>() {
                Ok(number) => *slot = number,
                Err(_) => self.mismatch(&found, ty),
            },
            (Ty::U8, Val::U8(slot)) => {
                match text.parse::<u64>().ok().and_then(|n| u8::try_from(n).ok()) {
                    Some(number) => *slot = number,
                    None => self.mismatch(&found, ty),
                }
            }
            (Ty::F64, Val::F64(slot)) => match text.parse::<f64>() {
                Ok(number) if number.is_finite() => *slot = number,
                _ => self.mismatch(&found, ty),
            },
            (Ty::F32, Val::F32(slot)) => match text.parse::<f32>() {
                Ok(number) if number.is_finite() => *slot = number,
                _ => self.mismatch(&found, ty),
            },
            (other, _) => self.mismatch("number", other),
        }
    }
}

/// `base64.StdEncoding.Decode`; `Err` is the `CorruptInputError` offset.
fn base64_decode(src: &[u8]) -> Result<Vec<u8>, usize> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let value = |byte: u8| TABLE.iter().position(|&entry| entry == byte);
    let mut out = Vec::with_capacity(src.len() / 4 * 3);
    let mut si = 0_usize;
    while si < src.len() {
        let mut quantum = [0_u32; 4];
        let mut j = 0_usize;
        let mut padded = 0_usize;
        while j < 4 {
            if si == src.len() {
                if j == 0 {
                    return Ok(out);
                }
                return Err(si - j);
            }
            let byte = src[si];
            si += 1;
            if let Some(number) = value(byte) {
                quantum[j] = u32::try_from(number).unwrap_or(0);
                j += 1;
                continue;
            }
            if byte == b'\n' || byte == b'\r' {
                continue;
            }
            if byte != b'=' {
                return Err(si - 1);
            }
            match j {
                0 | 1 => return Err(si - 1),
                2 => {
                    // Skip newlines between the two padding characters.
                    while si < src.len() && (src[si] == b'\n' || src[si] == b'\r') {
                        si += 1;
                    }
                    if si == src.len() {
                        return Err(src.len());
                    }
                    if src[si] != b'=' {
                        return Err(si - 1);
                    }
                    si += 1;
                }
                _ => {}
            }
            while si < src.len() && (src[si] == b'\n' || src[si] == b'\r') {
                si += 1;
            }
            if si < src.len() {
                return Err(si);
            }
            padded = 4 - j;
            break;
        }
        let word = (quantum[0] << 18) | (quantum[1] << 12) | (quantum[2] << 6) | quantum[3];
        let [_, high, middle, low] = word.to_be_bytes();
        let bytes = [high, middle, low];
        let take = if padded == 0 { 3 } else { 3 - padded };
        out.extend_from_slice(&bytes[..take]);
        if padded > 0 {
            return Ok(out);
        }
    }
    Ok(out)
}

// ------------------------------------------------------------- public API

/// A decoded `db.Memory`, with the zone of every timestamp kept.
#[derive(Clone, Debug)]
pub struct ClientMemory {
    pub id: String,
    pub content: String,
    pub scope: String,
    pub metadata: Option<BTreeMap<String, String>>,
    /// `None` is a nil slice, `Some(vec![])` an empty one.
    pub embedding: Option<Vec<f32>>,
    pub embedding_source: String,
    pub embedding_model: String,
    pub embedding_quantization: String,
    pub content_hash: String,
    pub created_at: GoTime,
    pub updated_at: GoTime,
    pub created_by: String,
    pub updated_by: String,
    pub created_session: String,
    pub updated_session: String,
    pub consolidation_status: String,
    pub consolidated_into_id: String,
    pub importance: f64,
    pub valid_from: Option<GoTime>,
    pub valid_to: Option<GoTime>,
    pub superseded_by: String,
    pub kind: String,
    pub review_status: String,
    pub decay_factor: f64,
    pub retired_at: Option<GoTime>,
    pub tier: String,
    pub expires_at: Option<GoTime>,
    pub access_count: i64,
    pub last_access: Option<GoTime>,
    pub prev_access: Option<GoTime>,
}

/// A decoded `db.DeletedMemory`.
#[derive(Clone, Debug)]
pub struct ClientDelete {
    pub id: String,
    pub deleted_at: GoTime,
}

/// A decoded `ChangesResponse`; a `None` memory is a JSON `null` element
/// (a nil pointer, which the shipped client dereferences).
#[derive(Clone, Debug)]
pub struct Changes {
    pub memories: Vec<Option<ClientMemory>>,
    pub deleted: Vec<ClientDelete>,
    pub server_time: GoTime,
    pub next_cursor: String,
}

/// A decoded `db.RelayBlob`.
#[derive(Clone, Debug)]
pub struct RelayBlob {
    pub id: String,
    pub updated_at: GoTime,
    pub blob: Vec<u8>,
}

/// A decoded `RelayResponse`.
#[derive(Clone, Debug)]
pub struct RelayPage {
    pub blobs: Vec<RelayBlob>,
    pub server_time: GoTime,
}

/// The counters of a decoded `ApplyResult`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ApplyCounts {
    pub applied: i64,
    pub deleted: i64,
}

/// A decoded `relayPayload`.
#[derive(Clone, Debug)]
pub struct RelayPayload {
    pub memory: Option<ClientMemory>,
    pub deleted: Option<ClientDelete>,
}

fn field<'a>(shape: &StructTy, values: &'a [Val], name: &str) -> &'a Val {
    let index = shape
        .fields
        .iter()
        .position(|(field, _)| *field == name)
        .unwrap_or(0);
    &values[index]
}

fn string_of(shape: &StructTy, values: &[Val], name: &str) -> String {
    match field(shape, values, name) {
        Val::Str(text) => text.clone(),
        _ => String::new(),
    }
}

fn time_of(shape: &StructTy, values: &[Val], name: &str) -> GoTime {
    match field(shape, values, name) {
        Val::Time(time) => time.clone(),
        _ => GoTime::zero(),
    }
}

fn time_ptr_of(shape: &StructTy, values: &[Val], name: &str) -> Option<GoTime> {
    match field(shape, values, name) {
        Val::TimePtr(time) => time.clone(),
        _ => None,
    }
}

fn float_of(shape: &StructTy, values: &[Val], name: &str) -> f64 {
    match field(shape, values, name) {
        Val::F64(number) => *number,
        _ => 0.0,
    }
}

fn memory_from(values: &[Val]) -> ClientMemory {
    let shape = &MEMORY;
    ClientMemory {
        id: string_of(shape, values, "id"),
        content: string_of(shape, values, "content"),
        scope: string_of(shape, values, "scope"),
        metadata: match field(shape, values, "metadata") {
            Val::Map(map) => map.clone(),
            _ => None,
        },
        embedding: match field(shape, values, "embedding") {
            Val::Slice(Some(items)) => Some(
                items
                    .iter()
                    .map(|item| {
                        if let Val::F32(number) = item {
                            *number
                        } else {
                            0.0
                        }
                    })
                    .collect(),
            ),
            _ => None,
        },
        embedding_source: string_of(shape, values, "embedding_source"),
        embedding_model: string_of(shape, values, "embedding_model"),
        embedding_quantization: string_of(shape, values, "embedding_quantization"),
        content_hash: string_of(shape, values, "content_hash"),
        created_at: time_of(shape, values, "created_at"),
        updated_at: time_of(shape, values, "updated_at"),
        created_by: string_of(shape, values, "created_by"),
        updated_by: string_of(shape, values, "updated_by"),
        created_session: string_of(shape, values, "created_session"),
        updated_session: string_of(shape, values, "updated_session"),
        consolidation_status: string_of(shape, values, "consolidation_status"),
        consolidated_into_id: string_of(shape, values, "consolidated_into_id"),
        importance: float_of(shape, values, "importance"),
        valid_from: time_ptr_of(shape, values, "valid_from"),
        valid_to: time_ptr_of(shape, values, "valid_to"),
        superseded_by: string_of(shape, values, "superseded_by"),
        kind: string_of(shape, values, "kind"),
        review_status: string_of(shape, values, "review_status"),
        decay_factor: float_of(shape, values, "decay_factor"),
        retired_at: time_ptr_of(shape, values, "retired_at"),
        tier: string_of(shape, values, "tier"),
        expires_at: time_ptr_of(shape, values, "expires_at"),
        access_count: match field(shape, values, "access_count") {
            Val::Int(number) => *number,
            _ => 0,
        },
        last_access: time_ptr_of(shape, values, "last_access"),
        prev_access: time_ptr_of(shape, values, "prev_access"),
    }
}

fn delete_from(values: &[Val]) -> ClientDelete {
    ClientDelete {
        id: string_of(&DELETED, values, "id"),
        deleted_at: time_of(&DELETED, values, "deleted_at"),
    }
}

fn pointer_memory(value: &Val) -> Option<ClientMemory> {
    match value {
        Val::Ptr(Some(inner)) => match &**inner {
            Val::Struct(values) => Some(memory_from(values)),
            _ => None,
        },
        _ => None,
    }
}

/// Runs `body` on a thread with room for the 10 000 levels of nesting that
/// `encoding/json` accepts.
fn with_stack<T: Send>(body: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(256 << 20)
            .spawn_scoped(scope, body)
            .map_or_else(
                |_| unreachable_stack(),
                |handle| handle.join().unwrap_or_else(|_| unreachable_stack()),
            )
    })
}

fn unreachable_stack<T>() -> T {
    std::process::abort()
}

/// The first value of a (possibly cut-off) response body, as
/// `json.NewDecoder(r).Decode` reads it. `stop` is Go's text of the read
/// error that ended the body early, when there was one.
fn read_value<'a>(body: &'a [u8], stop: Option<&str>) -> Result<&'a [u8], String> {
    let start = body
        .iter()
        .position(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
        .unwrap_or(body.len());
    let tail = &body[start..];
    match first_value(tail) {
        Ok(end) => Ok(&tail[..end]),
        Err(Fail::Syntax(message)) => Err(message),
        Err(Fail::Eof) => Err(stop.map_or_else(
            || {
                if tail.is_empty() {
                    "EOF".to_owned()
                } else {
                    "unexpected EOF".to_owned()
                }
            },
            str::to_owned,
        )),
    }
}

fn parse(value: &[u8]) -> Node {
    Parser {
        bytes: value,
        at: 0,
    }
    .value()
}

/// Decodes `value` into the top-level struct `shape`.
fn into_struct(value: &[u8], shape: &'static StructTy) -> (Result<(), String>, Vec<Val>) {
    let node = parse(value);
    let ty = Ty::Struct(shape);
    let mut target = zero(ty);
    let mut decoder = Decoder::new();
    let outcome = decoder.value(&node, ty, &mut target);
    let result = match outcome {
        Err(message) => Err(message),
        Ok(()) => decoder.saved.map_or(Ok(()), Err),
    };
    let Val::Struct(values) = target else {
        return (result, Vec::new());
    };
    (result, values)
}

/// `Client.Changes`' decode of `/api/sync/changes`; the error is the text after
/// `decode remote response: `.
///
/// # Errors
/// Go's text for a read, syntax or type failure.
pub fn decode_changes(body: &[u8], stop: Option<&str>) -> Result<Changes, String> {
    let value = read_value(body, stop)?;
    with_stack(|| {
        let (result, values) = into_struct(value, &CHANGES);
        result?;
        let memories = match field(&CHANGES, &values, "memories") {
            Val::Slice(Some(items)) => items.iter().map(pointer_memory).collect(),
            _ => Vec::new(),
        };
        let deleted = match field(&CHANGES, &values, "deleted") {
            Val::Slice(Some(items)) => items
                .iter()
                .map(|item| match item {
                    Val::Struct(values) => delete_from(values),
                    _ => ClientDelete {
                        id: String::new(),
                        deleted_at: GoTime::zero(),
                    },
                })
                .collect(),
            _ => Vec::new(),
        };
        Ok(Changes {
            memories,
            deleted,
            server_time: time_of(&CHANGES, &values, "server_time"),
            next_cursor: string_of(&CHANGES, &values, "next_cursor"),
        })
    })
}

/// The decode of `GET /api/sync/relay`.
///
/// # Errors
/// Go's text for a read, syntax or type failure.
pub fn decode_relay_page(body: &[u8], stop: Option<&str>) -> Result<RelayPage, String> {
    let value = read_value(body, stop)?;
    with_stack(|| {
        let (result, values) = into_struct(value, &RELAY_RESPONSE);
        result?;
        let blobs = match field(&RELAY_RESPONSE, &values, "blobs") {
            Val::Slice(Some(items)) => items
                .iter()
                .map(|item| match item {
                    Val::Struct(values) => RelayBlob {
                        id: string_of(&RELAY_BLOB, values, "id"),
                        updated_at: time_of(&RELAY_BLOB, values, "updated_at"),
                        blob: match field(&RELAY_BLOB, values, "blob") {
                            Val::Bytes(Some(bytes)) => bytes.clone(),
                            _ => Vec::new(),
                        },
                    },
                    _ => RelayBlob {
                        id: String::new(),
                        updated_at: GoTime::zero(),
                        blob: Vec::new(),
                    },
                })
                .collect(),
            _ => Vec::new(),
        };
        Ok(RelayPage {
            blobs,
            server_time: time_of(&RELAY_RESPONSE, &values, "server_time"),
        })
    })
}

/// The decode of `POST /api/sync/relay`: `(stored, skipped)`.
///
/// # Errors
/// Go's text for a read, syntax or type failure.
pub fn decode_relay_stored(body: &[u8], stop: Option<&str>) -> Result<i64, String> {
    let value = read_value(body, stop)?;
    with_stack(|| {
        let (result, values) = into_struct(value, &RELAY_PUSH_RESULT);
        result?;
        Ok(match field(&RELAY_PUSH_RESULT, &values, "stored") {
            Val::Int(number) => *number,
            _ => 0,
        })
    })
}

/// `ApplyResult.UnmarshalJSON` through the stream decoder.
///
/// # Errors
/// Go's text for a read or syntax failure, or for a value that is neither an
/// object nor `null`.
pub fn decode_apply_counts(body: &[u8], stop: Option<&str>) -> Result<ApplyCounts, String> {
    let value = read_value(body, stop)?;
    with_stack(|| {
        let node = parse(value);
        let members = match node {
            Node::Null => return Ok(ApplyCounts::default()),
            Node::Object(members) => members,
            Node::Array(_) => {
                return Err(unmarshal_into_raw_map("array"));
            }
            Node::Str { .. } => return Err(unmarshal_into_raw_map("string")),
            Node::Number(_) => return Err(unmarshal_into_raw_map("number")),
            Node::Bool => return Err(unmarshal_into_raw_map("bool")),
        };
        let get = |snake: &str, camel: &str| -> i64 {
            let found = members
                .iter()
                .rev()
                .find(|(key, _)| key == snake)
                .or_else(|| members.iter().rev().find(|(key, _)| key == camel));
            match found {
                Some((_, Node::Number(text))) => text.parse::<i64>().unwrap_or(0),
                _ => 0,
            }
        };
        Ok(ApplyCounts {
            applied: get("applied", "applied"),
            deleted: get("deleted", "deleted"),
        })
    })
}

fn unmarshal_into_raw_map(found: &str) -> String {
    let _ = &COUNT_ERROR;
    format!("json: cannot unmarshal {found} into Go value of type map[string]jsontext.Value")
}

/// The `{"error", "code"}` pair of an error body; the shipped decoder ignores
/// its own failure, so whatever was decoded before it counts.
#[must_use]
pub fn decode_api_error(body: &[u8]) -> (String, String) {
    let Ok(value) = read_value(body, None) else {
        return (String::new(), String::new());
    };
    with_stack(|| {
        let (_, values) = into_struct(value, &API_ERROR);
        (
            string_of(&API_ERROR, &values, "error"),
            string_of(&API_ERROR, &values, "code"),
        )
    })
}

/// `json.Unmarshal` of a decrypted relay payload.
///
/// # Errors
/// Go's text for a syntax or type failure.
pub fn decode_relay_payload(payload: &[u8]) -> Result<RelayPayload, String> {
    check_valid(payload)?;
    with_stack(|| {
        let (result, values) = into_struct(payload, &RELAY_PAYLOAD);
        result?;
        let memory = pointer_memory(field(&RELAY_PAYLOAD, &values, "memory"));
        let deleted = match field(&RELAY_PAYLOAD, &values, "deleted") {
            Val::Ptr(Some(inner)) => match &**inner {
                Val::Struct(values) => Some(delete_from(values)),
                _ => None,
            },
            _ => None,
        };
        Ok(RelayPayload { memory, deleted })
    })
}

/// `json.Unmarshal` of a stored metadata column into `map[string]string`.
pub(crate) fn decode_metadata(text: &str) -> Result<Option<BTreeMap<String, String>>, String> {
    check_valid(text.as_bytes())?;
    with_stack(|| {
        let node = parse(text.as_bytes());
        let mut target = zero(Ty::MapStr);
        let mut decoder = Decoder::new();
        decoder.value(&node, Ty::MapStr, &mut target)?;
        if let Some(message) = decoder.saved {
            return Err(message);
        }
        Ok(match target {
            Val::Map(map) => map,
            _ => None,
        })
    })
}

// --------------------------------------------------------------- encoding

/// `json.Marshal` of a string (HTML escaping on, invalid UTF-8 replaced).
#[must_use]
pub fn quote(text: &str) -> String {
    quote_bytes(text.as_bytes())
}

/// Like [`quote`] for bytes that may be invalid UTF-8.
#[must_use]
pub fn quote_bytes(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let text = go_lossy(bytes);
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `json.Marshal` of a `float64`.
#[must_use]
pub fn float64(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude != 0.0 && !(1e-6..1e21).contains(&magnitude) {
        let rendered = format!("{value:e}");
        if let Some((mantissa, exponent)) = rendered.split_once('e') {
            return match exponent.strip_prefix('-') {
                Some(digits) => format!("{mantissa}e-{digits}"),
                None => format!("{mantissa}e+{exponent}"),
            };
        }
    }
    format!("{value}")
}

/// A memory read from the local store for pushing (the shipped "lite" scan,
/// so no vector, entities or evidence).
#[derive(Clone, Debug)]
pub struct LocalMemory {
    pub id: String,
    pub content: String,
    pub scope: String,
    pub metadata: Option<BTreeMap<String, String>>,
    pub created_at: GoTime,
    pub updated_at: GoTime,
    pub created_by: String,
    pub updated_by: String,
    pub created_session: String,
    pub updated_session: String,
    pub consolidation_status: String,
    pub consolidated_into_id: String,
    pub importance: f64,
    pub valid_from: Option<GoTime>,
    pub valid_to: Option<GoTime>,
    pub superseded_by: String,
    pub kind: String,
    pub review_status: String,
    pub decay_factor: f64,
    pub retired_at: Option<GoTime>,
    pub tier: String,
    pub expires_at: Option<GoTime>,
    pub access_count: i64,
    pub last_access: Option<GoTime>,
    pub prev_access: Option<GoTime>,
}

fn string_member(out: &mut Vec<String>, name: &str, value: &str, omit_empty: bool) {
    if !(omit_empty && value.is_empty()) {
        out.push(format!("{}:{}", quote(name), quote(value)));
    }
}

fn time_member(out: &mut Vec<String>, name: &str, value: &GoTime) {
    out.push(format!(
        "{}:{}",
        quote(name),
        quote(&value.to_rfc3339_nano())
    ));
}

fn time_ptr_member(out: &mut Vec<String>, name: &str, value: Option<&GoTime>) {
    if let Some(time) = value {
        time_member(out, name, time);
    }
}

impl LocalMemory {
    /// `json.Marshal` of the `db.Memory` this row scans into.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut members = Vec::new();
        string_member(&mut members, "id", &self.id, false);
        string_member(&mut members, "content", &self.content, false);
        string_member(&mut members, "scope", &self.scope, false);
        members.push(format!(
            "{}:{}",
            quote("metadata"),
            self.metadata.as_ref().map_or_else(
                || "null".to_owned(),
                |map| {
                    let items: Vec<String> = map
                        .iter()
                        .map(|(key, value)| format!("{}:{}", quote(key), quote(value)))
                        .collect();
                    format!("{{{}}}", items.join(","))
                }
            )
        ));
        time_member(&mut members, "created_at", &self.created_at);
        time_member(&mut members, "updated_at", &self.updated_at);
        string_member(&mut members, "created_by", &self.created_by, true);
        string_member(&mut members, "updated_by", &self.updated_by, true);
        string_member(&mut members, "created_session", &self.created_session, true);
        string_member(&mut members, "updated_session", &self.updated_session, true);
        string_member(
            &mut members,
            "consolidation_status",
            &self.consolidation_status,
            false,
        );
        string_member(
            &mut members,
            "consolidated_into_id",
            &self.consolidated_into_id,
            true,
        );
        members.push(format!(
            "{}:{}",
            quote("importance"),
            float64(self.importance)
        ));
        time_ptr_member(&mut members, "valid_from", self.valid_from.as_ref());
        time_ptr_member(&mut members, "valid_to", self.valid_to.as_ref());
        string_member(&mut members, "superseded_by", &self.superseded_by, true);
        string_member(&mut members, "kind", &self.kind, true);
        string_member(&mut members, "review_status", &self.review_status, false);
        if self.decay_factor != 0.0 {
            members.push(format!(
                "{}:{}",
                quote("decay_factor"),
                float64(self.decay_factor)
            ));
        }
        time_ptr_member(&mut members, "retired_at", self.retired_at.as_ref());
        string_member(&mut members, "tier", &self.tier, false);
        time_ptr_member(&mut members, "expires_at", self.expires_at.as_ref());
        members.push(format!("{}:{}", quote("access_count"), self.access_count));
        time_ptr_member(&mut members, "last_access", self.last_access.as_ref());
        time_ptr_member(&mut members, "prev_access", self.prev_access.as_ref());
        format!("{{{}}}", members.join(","))
    }
}

/// `json.Marshal` of a `db.DeletedMemory`.
#[must_use]
pub fn delete_json(id: &str, deleted_at: &GoTime) -> String {
    format!(
        "{{{}:{},{}:{}}}",
        quote("id"),
        quote(id),
        quote("deleted_at"),
        quote(&deleted_at.to_rfc3339_nano())
    )
}

/// `json.Marshal` of the `/api/sync/apply` request body; a batch without
/// entries of one kind marshals that slice as `null`.
#[must_use]
pub fn apply_body(memories: &[LocalMemory], deleted: &[(String, GoTime)]) -> String {
    let memory_list = if memories.is_empty() {
        "null".to_owned()
    } else {
        format!(
            "[{}]",
            memories
                .iter()
                .map(LocalMemory::to_json)
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    let delete_list = if deleted.is_empty() {
        "null".to_owned()
    } else {
        format!(
            "[{}]",
            deleted
                .iter()
                .map(|(id, at)| delete_json(id, at))
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    format!("{{\"memories\":{memory_list},\"deleted\":{delete_list}}}")
}

/// `json.Marshal` of the plaintext carried inside one relay blob.
#[must_use]
pub fn relay_payload_json(
    memory: Option<&LocalMemory>,
    deleted: Option<(&str, &GoTime)>,
) -> String {
    if let Some(memory) = memory {
        return format!("{{\"memory\":{}}}", memory.to_json());
    }
    deleted.map_or_else(
        || "{}".to_owned(),
        |(id, at)| format!("{{\"deleted\":{}}}", delete_json(id, at)),
    )
}

/// `json.Marshal` of a `POST /api/sync/relay` body.
#[must_use]
pub fn relay_body(blobs: &[(String, GoTime, Vec<u8>)]) -> String {
    use base64::Engine as _;
    let items: Vec<String> = blobs
        .iter()
        .map(|(id, at, bytes)| {
            format!(
                "{{{}:{},{}:{},{}:{}}}",
                quote("id"),
                quote(id),
                quote("updated_at"),
                quote(&at.to_rfc3339_nano()),
                quote("blob"),
                quote(&base64::engine::general_purpose::STANDARD.encode(bytes))
            )
        })
        .collect();
    if items.is_empty() {
        "{\"blobs\":null}".to_owned()
    } else {
        format!("{{\"blobs\":[{}]}}", items.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_semantics_for_duplicate_and_folded_keys() {
        let body = br#"{"MEMORIES":[{"ID":"a","metadata":{"k":null,"k2":"v"},"Embedding":[],"importance":0.5}],
            "deleted":[{"id":"b","deleted_at":"2026-10-26T01:00:00.5Z"}],
            "server_time":"2026-10-26T01:02:03+02:00","next_cursor":"p2","next_cursor":"p3"}"#;
        let page = decode_changes(body, None).unwrap();
        assert_eq!(page.next_cursor, "p3");
        let memory = page.memories[0].as_ref().unwrap();
        assert_eq!(memory.id, "a");
        assert_eq!(memory.metadata.as_ref().unwrap()["k"], "");
        assert_eq!(memory.embedding, Some(Vec::new()));
        assert_eq!(page.deleted[0].id, "b");
        assert_eq!(
            page.server_time.to_go_string(),
            "2026-10-26 01:02:03 +0200 +0200"
        );
    }

    #[test]
    fn first_type_error_wins_and_time_errors_abort() {
        let error = |body: &str| decode_changes(body.as_bytes(), None).unwrap_err();
        assert_eq!(
            error(r#"{"memories":[{"id":5,"content":7}]}"#),
            "json: cannot unmarshal number into Go struct field ChangesResponse.memories.0.id of type string"
        );
        assert_eq!(
            error(r#"{"memories":[{"id":5,"created_at":"bad"}]}"#),
            r#"parsing time "bad" as "2006-01-02T15:04:05Z07:00": cannot parse "bad" as "2006""#
        );
        assert_eq!(
            error("[]"),
            "json: cannot unmarshal array into Go value of type syncclient.ChangesResponse"
        );
        assert_eq!(error(""), "EOF");
        assert_eq!(error("{\"a\":"), "unexpected EOF");
        assert!(decode_changes(b"null", None).is_ok());
    }

    #[test]
    fn apply_counts_follow_the_custom_unmarshaler() {
        let counts =
            decode_apply_counts(br#"{"applied":3,"deleted":"x","skipped":1.5}"#, None).unwrap();
        assert_eq!(
            counts,
            ApplyCounts {
                applied: 3,
                deleted: 0
            }
        );
        assert_eq!(
            decode_apply_counts(b"[1]", None).unwrap_err(),
            "json: cannot unmarshal array into Go value of type map[string]jsontext.Value"
        );
        assert_eq!(
            decode_apply_counts(b"null", None).unwrap(),
            ApplyCounts::default()
        );
    }

    #[test]
    fn api_errors_keep_what_decoded_before_a_type_error() {
        assert_eq!(
            decode_api_error(br#"{"error":5,"code":"c"}"#),
            (String::new(), "c".into())
        );
        assert_eq!(
            decode_api_error(b"{\"error\":\"cut"),
            (String::new(), String::new())
        );
    }

    #[test]
    fn push_bodies_marshal_like_encoding_json() {
        assert_eq!(float64(0.5), "0.5");
        assert_eq!(float64(1.0), "1");
        assert_eq!(float64(1e-7), "1e-7");
        assert_eq!(float64(1e21), "1e+21");
        assert_eq!(
            quote("a<b>&\u{2028}\u{8}"),
            "\"a\\u003cb\\u003e\\u0026\\u2028\\b\""
        );
        assert_eq!(apply_body(&[], &[]), "{\"memories\":null,\"deleted\":null}");
    }
}
