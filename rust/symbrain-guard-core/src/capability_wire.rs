//! Go-compatible wire details kept separate from the cryptographic contract.

use base64::{
    Engine, alphabet,
    engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig},
};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{MapAccess, Visitor},
};
use serde_json::value::RawValue;
use std::fmt;

use crate::capability::{Claims, Token};

pub(crate) fn base64_engine() -> GeneralPurpose {
    GeneralPurpose::new(
        &alphabet::URL_SAFE,
        GeneralPurposeConfig::new()
            .with_encode_padding(false)
            .with_decode_padding_mode(DecodePaddingMode::RequireNone)
            .with_decode_allow_trailing_bits(true),
    )
}

pub(crate) fn decode_base64(input: &str) -> Result<Vec<u8>, base64::DecodeError> {
    let bytes: Vec<u8> = input
        .bytes()
        .filter(|b| !matches!(b, b'\r' | b'\n'))
        .collect();
    base64_engine().decode(bytes)
}

pub(crate) fn canonical_json<T: Serialize>(value: &T) -> String {
    // Only concrete claims/token structs reach this helper. Go's default
    // encoder additionally escapes HTML and JavaScript line separators.
    serde_json::to_string(value)
        .expect("token fields are JSON serializable")
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

// Keep duplicate object fields in wire order: encoding/json merges repeated
// claims objects, ignores scalar null, and accepts case-insensitive field names.
#[derive(Default)]
struct Object(Vec<(String, Box<RawValue>)>);

impl<'de> Deserialize<'de> for Object {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor;
        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = Object;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("object or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Object, E> {
                Ok(Object::default())
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Object, M::Error> {
                let mut fields = Vec::new();
                while let Some(field) = map.next_entry()? {
                    fields.push(field);
                }
                Ok(Object(fields))
            }
        }
        deserializer.deserialize_any(ObjectVisitor)
    }
}

fn fold(field: &str) -> String {
    field
        .chars()
        .map(|c| match c {
            '\u{017f}' => 's',
            '\u{212a}' => 'k',
            _ => c.to_ascii_lowercase(),
        })
        .collect()
}

fn merge_claims(
    claims: &mut Claims,
    scope_storage: &mut Vec<String>,
    raw: &str,
) -> Result<(), serde_json::Error> {
    let object: Object = serde_json::from_str(raw)?;
    for (field, value) in object.0 {
        let raw = value.get();
        let name = fold(&field);
        if raw == "null" && name != "scope" {
            continue;
        }
        match name.as_str() {
            "sub" => claims.subject = serde_json::from_str(raw)?,
            "purpose" => claims.purpose = serde_json::from_str(raw)?,
            "jti" => claims.jti = serde_json::from_str(raw)?,
            "iat" => claims.iat = go_integer(raw)?,
            "exp" => claims.exp = go_integer(raw)?,
            "scope" => {
                let entries: Option<Vec<Option<String>>> = serde_json::from_str(raw)?;
                claims.scope = entries.map(|entries| {
                    // Go reuses slice backing elements across duplicate fields;
                    // null leaves an existing string unchanged, even after a
                    // shorter array. Empty arrays explicitly discard storage.
                    if entries.is_empty() {
                        scope_storage.clear();
                    }
                    let len = entries.len();
                    scope_storage.resize(scope_storage.len().max(len), String::new());
                    for (index, entry) in entries.into_iter().enumerate() {
                        if let Some(value) = entry {
                            scope_storage[index] = value;
                        }
                    }
                    scope_storage[..len].to_vec()
                });
                if claims.scope.is_none() {
                    scope_storage.clear();
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn go_integer(raw: &str) -> Result<i64, serde_json::Error> {
    // Go accepts integer negative zero, but not floating-point/exponent forms.
    if raw == "-0" {
        Ok(0)
    } else {
        serde_json::from_str(raw)
    }
}

pub(crate) fn decode_token(raw: &[u8]) -> Result<Token, serde_json::Error> {
    let json = repair_json_strings(raw);
    let object: Object = serde_json::from_str(&json)?;
    let mut token = Token::default();
    let mut scope_storage = Vec::new();
    for (field, value) in object.0 {
        match fold(&field).as_str() {
            "claims" => merge_claims(&mut token.claims, &mut scope_storage, value.get())?,
            "sig" if value.get() != "null" => token.signature = serde_json::from_str(value.get())?,
            _ => {}
        }
    }
    Ok(token)
}

fn hex_quad(bytes: &[u8]) -> Option<u16> {
    let text = std::str::from_utf8(bytes).ok()?;
    u16::from_str_radix(text, 16).ok()
}

// encoding/json replaces each invalid UTF-8 byte and unpaired UTF-16 surrogate
// inside strings with U+FFFD. Preserve escapes and valid surrogate pairs so the
// normal JSON parser still rejects malformed syntax and field types.
fn repair_json_strings(raw: &[u8]) -> String {
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
