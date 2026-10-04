//! `CoreKit` reflection semantics in explicit Go field declaration order.
use super::{BrainConfig, Sources};
use crate::{GoText, config::format_go_quoted_bytes, go_path};
use toml_edit::{DocumentMut, Item, Value};

#[derive(Clone, Copy)]
enum Kind {
    Text,
    Bool { pointer: bool },
    Int,
}
struct Field {
    path: &'static str,
    kind: Kind,
}
const POINTER: Kind = Kind::Bool { pointer: true };
const PLAIN: Kind = Kind::Bool { pointer: false };
const FIELDS: [Field; 13] = [
    Field {
        path: "default_profile",
        kind: Kind::Text,
    },
    Field {
        path: "audit.enabled",
        kind: POINTER,
    },
    Field {
        path: "audit.verbose",
        kind: PLAIN,
    },
    Field {
        path: "gateway.identity_injection",
        kind: POINTER,
    },
    Field {
        path: "updatecheck.enabled",
        kind: POINTER,
    },
    Field {
        path: "servers.vault.binary_path",
        kind: Kind::Text,
    },
    Field {
        path: "servers.operate.binary_path",
        kind: Kind::Text,
    },
    Field {
        path: "servers.scope.binary_path",
        kind: Kind::Text,
    },
    Field {
        path: "patterns.enabled",
        kind: POINTER,
    },
    Field {
        path: "patterns.promotion_threshold",
        kind: Kind::Int,
    },
    Field {
        path: "modules.browse",
        kind: PLAIN,
    },
    Field {
        path: "modules.operate",
        kind: PLAIN,
    },
    Field {
        path: "modules.scope",
        kind: PLAIN,
    },
];
enum Scalar {
    Text(Vec<u8>),
    Bool(bool),
    Int(i64),
}
fn node<'a>(document: &'a DocumentMut, path: &str) -> Option<&'a Item> {
    let mut parts = path.split('.');
    let mut item = document.get(parts.next()?)?;
    for part in parts {
        item = item.as_table_like()?.get(part)?;
    }
    Some(item)
}
fn is_zero(item: &Item) -> bool {
    match item.as_value() {
        Some(Value::String(value)) => value.value().is_empty(),
        Some(Value::Integer(value)) => *value.value() == 0,
        Some(Value::Float(value)) => *value.value() == 0.0,
        Some(Value::Boolean(value)) => !value.value(),
        _ => item.is_none(),
    }
}
fn type_name(item: &Item) -> &'static str {
    match item {
        Item::Table(_) | Item::Value(Value::InlineTable(_)) => "map[string]interface {}",
        Item::ArrayOfTables(_) => "[]map[string]interface {}",
        Item::Value(Value::Array(_)) => "[]interface {}",
        Item::Value(Value::Datetime(_)) => "time.Time",
        Item::Value(Value::Float(_)) => "float64",
        Item::Value(Value::Integer(_)) => "int64",
        Item::Value(Value::Boolean(_)) => "bool",
        Item::Value(Value::String(_)) => "string",
        Item::None => "<nil>",
    }
}
fn convert(item: &Item, kind: Kind) -> Result<Scalar, GoText> {
    if let Some(text) = item.as_str() {
        return convert_bytes(text.as_bytes(), kind);
    }
    match kind {
        Kind::Text => Err(format!("cannot convert {} to string", type_name(item)).into()),
        Kind::Bool { .. } => item
            .as_bool()
            .map(Scalar::Bool)
            .ok_or_else(|| format!("cannot convert {} to bool", type_name(item)).into()),
        Kind::Int => item
            .as_integer()
            .map(Scalar::Int)
            .or_else(|| item.as_float().map(|v| Scalar::Int(go_float_to_int(v))))
            .ok_or_else(|| format!("cannot convert {} to int", type_name(item)).into()),
    }
}
fn convert_bytes(bytes: &[u8], kind: Kind) -> Result<Scalar, GoText> {
    let quoted = || format_go_quoted_bytes(bytes);
    match kind {
        Kind::Text => Ok(Scalar::Text(bytes.to_vec())),
        Kind::Bool { .. } => parse_bool(bytes).map(Scalar::Bool).ok_or_else(|| {
            let q = quoted();
            format!("cannot parse {q} as bool: strconv.ParseBool: parsing {q}: invalid syntax")
                .into()
        }),
        Kind::Int => parse_int(bytes).map(Scalar::Int).map_err(|cause| {
            let q = quoted();
            format!("cannot parse {q} as int: strconv.ParseInt: parsing {q}: {cause}").into()
        }),
    }
}
/// Go strconv.ParseBool accepts exactly these aliases without trimming.
#[must_use]
pub fn parse_bool(bytes: &[u8]) -> Option<bool> {
    match bytes {
        b"1" | b"t" | b"T" | b"true" | b"TRUE" | b"True" => Some(true),
        b"0" | b"f" | b"F" | b"false" | b"FALSE" | b"False" => Some(false),
        _ => None,
    }
}
fn parse_int(original: &[u8]) -> Result<i64, &'static str> {
    let (negative, bytes) = match original.first() {
        Some(b'-') => (true, &original[1..]),
        Some(b'+') => (false, &original[1..]),
        _ => (false, original),
    };
    if bytes.is_empty() {
        return Err("invalid syntax");
    }
    let mut value = 0_u64;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return Err("invalid syntax");
        }
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(u64::from(byte - b'0')))
            .ok_or("value out of range")?;
    }
    let limit = u64::try_from(i64::MAX).expect("positive maximum") + u64::from(negative);
    if value > limit {
        return Err("value out of range");
    }
    if negative && value == limit {
        return Ok(i64::MIN);
    }
    let value = i64::try_from(value).map_err(|_| "value out of range")?;
    Ok(if negative { -value } else { value })
}
#[allow(clippy::cast_possible_truncation)]
fn go_float_to_int(value: f64) -> i64 {
    // Pinned Go SDK Cvt64Fto64 lowers to CVTTSD2SQ on AMD64 and
    // FCVTZSD on ARM64. The native six-platform gate must prove both.
    #[cfg(target_arch = "x86_64")]
    if !value.is_finite()
        || !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&value)
    {
        return i64::MIN;
    }
    value as i64
}
fn assign(config: &mut BrainConfig, index: usize, value: Scalar) {
    match (index, value) {
        (0, Scalar::Text(v)) => config.default_profile = v.into(),
        (1, Scalar::Bool(v)) => config.audit.enabled = v,
        (2, Scalar::Bool(v)) => config.audit.verbose = v,
        (3, Scalar::Bool(v)) => config.gateway.identity_injection = v,
        (4, Scalar::Bool(v)) => config.updatecheck.enabled = v,
        (5, Scalar::Text(v)) => config.servers.vault = v.into(),
        (6, Scalar::Text(v)) => config.servers.operate = v.into(),
        (7, Scalar::Text(v)) => config.servers.scope = v.into(),
        (8, Scalar::Bool(v)) => config.patterns.enabled = v,
        (9, Scalar::Int(v)) => config.patterns.promotion_threshold = v,
        (10, Scalar::Bool(v)) => config.modules.browse = v,
        (11, Scalar::Bool(v)) => config.modules.operate = v,
        (12, Scalar::Bool(v)) => config.modules.scope = v,
        _ => unreachable!("fixed Brain field table has a matching conversion kind"),
    }
}
pub(super) fn apply_document(
    config: &mut BrainConfig,
    document: &DocumentMut,
) -> Result<(), GoText> {
    for (index, field) in FIELDS.iter().enumerate() {
        let Some(item) = node(document, field.path) else {
            continue;
        };
        if !matches!(field.kind, Kind::Bool { pointer: true }) && is_zero(item) {
            continue;
        }
        let value = convert(item, field.kind).map_err(|mut error| {
            for part in field.path.rsplit('.') {
                error = error.with_prefix(&format!("field {part:?}: "));
            }
            error
        })?;
        assign(config, index, value);
    }
    Ok(())
}
pub(super) fn apply_environment(
    config: &mut BrainConfig,
    source: &impl Sources,
) -> Result<(), GoText> {
    for (index, field) in FIELDS.iter().enumerate() {
        let key = format!("SYMBRAIN_{}", field.path.replace('.', "_").to_uppercase());
        let Some(value) = source.environment(&key).filter(|v| !v.is_empty()) else {
            continue;
        };
        let value = convert_bytes(&go_path::os_bytes(&value), field.kind)
            .map_err(|error| error.with_prefix(&format!("env {key}: ")))?;
        assign(config, index, value);
    }
    Ok(())
}
