//! Byte-oriented TOML encoding and table tree manipulation matching BurntSushi/toml.

use std::collections::BTreeMap;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value};

use super::format;

#[derive(Clone, Debug, PartialEq)]
pub enum ConfigValue {
    String(Vec<u8>),
    Integer(i64),
    Boolean(bool),
    Float(f64),
    Datetime(String),
    Array(Vec<ConfigValue>),
    Table(ConfigTable),
    ArrayOfTables(Vec<ConfigTable>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigTable {
    pub entries: BTreeMap<Vec<u8>, ConfigValue>,
}

impl ConfigTable {
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn from_document(doc: &DocumentMut) -> Self {
        Self::from_table(doc.as_table())
    }

    pub fn from_table(table: &Table) -> Self {
        let mut entries = BTreeMap::new();
        for (k, item) in table {
            if let Some(val) = Self::convert_item(item) {
                entries.insert(super::lossless::from_lossless_str(k), val);
            }
        }
        Self { entries }
    }

    pub fn from_inline_table(table: &InlineTable) -> Self {
        let mut entries = BTreeMap::new();
        for (k, val) in table {
            entries.insert(
                super::lossless::from_lossless_str(k),
                Self::convert_value(val),
            );
        }
        Self { entries }
    }

    fn convert_item(item: &Item) -> Option<ConfigValue> {
        match item {
            Item::Table(t) => Some(ConfigValue::Table(Self::from_table(t))),
            Item::Value(Value::InlineTable(it)) => {
                Some(ConfigValue::Table(Self::from_inline_table(it)))
            }
            Item::Value(val) => Some(Self::convert_value(val)),
            Item::ArrayOfTables(a) => {
                let tables = a.iter().map(Self::from_table).collect();
                Some(ConfigValue::ArrayOfTables(tables))
            }
            Item::None => None,
        }
    }

    fn convert_value(val: &Value) -> ConfigValue {
        match val {
            Value::String(s) => ConfigValue::String(super::lossless::from_lossless_str(s.value())),
            Value::Integer(i) => ConfigValue::Integer(*i.value()),
            Value::Boolean(b) => ConfigValue::Boolean(*b.value()),
            Value::Float(f) => ConfigValue::Float(*f.value()),
            Value::Datetime(dt) => ConfigValue::Datetime(format_toml_datetime(dt.value())),
            Value::Array(a) => Self::convert_array(a),
            Value::InlineTable(it) => ConfigValue::Table(Self::from_inline_table(it)),
        }
    }

    fn convert_array(arr: &Array) -> ConfigValue {
        let is_all_inline_tables =
            !arr.is_empty() && arr.iter().all(|v| matches!(v, Value::InlineTable(_)));
        if is_all_inline_tables {
            let tables = arr
                .iter()
                .filter_map(|v| match v {
                    Value::InlineTable(it) => Some(Self::from_inline_table(it)),
                    _ => None,
                })
                .collect();
            ConfigValue::ArrayOfTables(tables)
        } else {
            let vals = arr.iter().map(Self::convert_value).collect();
            ConfigValue::Array(vals)
        }
    }
}

pub fn is_bare_key(piece: &[u8]) -> bool {
    !piece.is_empty()
        && piece
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub fn escape_quoted_bytes(bytes: &[u8], out: &mut Vec<u8>) {
    out.push(b'"');
    for &b in bytes {
        match b {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\x08' => out.extend_from_slice(b"\\b"),
            b'\t' => out.extend_from_slice(b"\\t"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\x0c' => out.extend_from_slice(b"\\f"),
            b'\r' => out.extend_from_slice(b"\\r"),
            0x00..=0x1F | 0x7F => {
                let hex = b"0123456789abcdef";
                out.extend_from_slice(&[
                    b'\\',
                    b'u',
                    b'0',
                    b'0',
                    hex[(b >> 4) as usize],
                    hex[(b & 0xf) as usize],
                ]);
            }
            other => out.push(other),
        }
    }
    out.push(b'"');
}

pub fn emit_key_piece(piece: &[u8], out: &mut Vec<u8>) {
    if is_bare_key(piece) {
        out.extend_from_slice(piece);
    } else {
        escape_quoted_bytes(piece, out);
    }
}

pub fn format_toml_float(f: f64) -> String {
    if f.is_nan() {
        if f.is_sign_negative() {
            "-nan".to_string()
        } else {
            "nan".to_string()
        }
    } else if f.is_infinite() {
        if f.is_sign_negative() {
            "-inf".to_string()
        } else {
            "inf".to_string()
        }
    } else if f == 0.0 {
        if f.is_sign_negative() {
            "-0.0".to_string()
        } else {
            "0.0".to_string()
        }
    } else if let Some(sci) = format::format_go_float_scientific(f) {
        sci
    } else {
        let s = if f.fract() == 0.0 {
            format!("{f:.0}")
        } else {
            f.to_string()
        };
        if !s.contains('.') && !s.contains('e') && !s.contains('E') {
            format!("{s}.0")
        } else {
            s
        }
    }
}

pub fn format_toml_datetime(dt: &toml_edit::Datetime) -> String {
    let date_str = dt.date.map(format::format_date_str);
    let time_str = dt.time.as_ref().map(format::format_time_str);

    match (date_str, time_str) {
        (Some(d), Some(t)) => {
            let zone = match dt.offset {
                Some(toml_edit::Offset::Custom { minutes }) => {
                    if minutes == 0 {
                        "Z".to_string()
                    } else {
                        let sign = if minutes >= 0 { '+' } else { '-' };
                        let abs = minutes.unsigned_abs();
                        format!("{sign}{:02}:{:02}", abs / 60, abs % 60)
                    }
                }
                Some(toml_edit::Offset::Z) => "Z".to_string(),
                None => String::new(),
            };
            format!("{d}T{t}{zone}")
        }
        (Some(d), None) => d,
        (None, Some(t)) => t,
        (None, None) => String::new(),
    }
}

fn emit_value(value: &ConfigValue, out: &mut Vec<u8>) {
    match value {
        ConfigValue::String(s) => escape_quoted_bytes(s, out),
        ConfigValue::Integer(i) => {
            out.extend_from_slice(i.to_string().as_bytes());
        }
        ConfigValue::Boolean(b) => {
            out.extend_from_slice(if *b { b"true" } else { b"false" });
        }
        ConfigValue::Float(f) => {
            out.extend_from_slice(format_toml_float(*f).as_bytes());
        }
        ConfigValue::Datetime(dt) => {
            out.extend_from_slice(dt.as_bytes());
        }
        ConfigValue::Array(arr) => {
            out.push(b'[');
            for (i, elem) in arr.iter().enumerate() {
                if i > 0 {
                    out.extend_from_slice(b", ");
                }
                emit_value(elem, out);
            }
            out.push(b']');
        }
        ConfigValue::Table(_) | ConfigValue::ArrayOfTables(_) => {}
    }
}

pub fn set_key_in_table(
    root: &mut ConfigTable,
    key_bytes: &[u8],
    value: ConfigValue,
) -> Result<(), String> {
    let parts: Vec<&[u8]> = key_bytes.split(|&b| b == b'.').collect();
    let mut current = root;
    for &part in &parts[..parts.len() - 1] {
        let next = current
            .entries
            .entry(part.to_vec())
            .or_insert_with(|| ConfigValue::Table(ConfigTable::new()));
        if let ConfigValue::Table(tbl) = next {
            current = tbl;
        } else {
            return Err(format!(
                "cannot set {}: {} is not a table",
                format::format_go_quoted_bytes(key_bytes),
                format::format_go_quoted_bytes(part)
            ));
        }
    }
    current
        .entries
        .insert(parts[parts.len() - 1].to_vec(), value);
    Ok(())
}

fn emit_table_header(parts: &[Vec<u8>], out: &mut Vec<u8>) {
    out.push(b'[');
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            out.push(b'.');
        }
        emit_key_piece(part, out);
    }
    out.extend_from_slice(b"]\n");
}

fn emit_array_header(parts: &[Vec<u8>], out: &mut Vec<u8>) {
    out.extend_from_slice(b"[[");
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            out.push(b'.');
        }
        emit_key_piece(part, out);
    }
    out.extend_from_slice(b"]]\n");
}

enum TomlTableEntry<'a> {
    Table(&'a ConfigTable),
    ArrayOfTables(&'a [ConfigTable]),
}

fn emit_table_recursive(table: &ConfigTable, prefix: &[Vec<u8>], out: &mut Vec<u8>) {
    let mut direct = Vec::new();
    let mut sub_tables = Vec::new();

    for (k, v) in &table.entries {
        match v {
            ConfigValue::Table(tbl) => sub_tables.push((k.clone(), TomlTableEntry::Table(tbl))),
            ConfigValue::ArrayOfTables(arr) => {
                sub_tables.push((k.clone(), TomlTableEntry::ArrayOfTables(arr)));
            }
            _ => direct.push((k.clone(), v)),
        }
    }

    let indent_depth = prefix.len();
    let indent = "  ".repeat(indent_depth);

    for (k, v) in direct {
        out.extend_from_slice(indent.as_bytes());
        emit_key_piece(&k, out);
        out.extend_from_slice(b" = ");
        emit_value(v, out);
        out.push(b'\n');
    }

    for (k, entry) in sub_tables {
        let mut full = prefix.to_vec();
        full.push(k);
        match entry {
            TomlTableEntry::Table(sub) => {
                if full.len() == 1 && !out.is_empty() {
                    out.push(b'\n');
                }
                let tbl_indent = "  ".repeat(full.len() - 1);
                out.extend_from_slice(tbl_indent.as_bytes());
                emit_table_header(&full, out);
                emit_table_recursive(sub, &full, out);
            }
            TomlTableEntry::ArrayOfTables(arr) => {
                for item in arr {
                    if !out.is_empty() {
                        out.push(b'\n');
                    }
                    let arr_indent = "  ".repeat(full.len() - 1);
                    out.extend_from_slice(arr_indent.as_bytes());
                    emit_array_header(&full, out);
                    emit_table_recursive(item, &full, out);
                }
            }
        }
    }
}

pub fn encode_document(table: &ConfigTable) -> Vec<u8> {
    let mut out = Vec::new();
    emit_table_recursive(table, &[], &mut out);
    out
}
