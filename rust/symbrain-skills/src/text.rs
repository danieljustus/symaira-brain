//! Byte-owned Go strings for Skills presentation; no pathname normalization.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;
use std::{fmt, ops::Deref, path::Path};

/// Go string bytes, retaining invalid bytes separately from literal U+FFFD.
/// Human writers use bytes; JSON encoders replace each invalid byte at output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GoText {
    bytes: Vec<u8>,
    decoded: String,
}
impl GoText {
    /// Owns raw bytes without repairing the stored value.
    /// # Panics
    /// Only if an internal validated UTF-8 prefix invariant is violated.
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut decoded = String::new();
        let mut rest = bytes;
        while !rest.is_empty() {
            match std::str::from_utf8(rest) {
                Ok(text) => {
                    decoded.push_str(text);
                    break;
                }
                Err(error) => {
                    let end = error.valid_up_to();
                    decoded.push_str(std::str::from_utf8(&rest[..end]).expect("validated prefix"));
                    decoded.push('\u{fffd}');
                    rest = &rest[end + 1..];
                }
            }
        }
        Self {
            bytes: bytes.to_vec(),
            decoded,
        }
    }
    /// Retains Unix native bytes / Windows WTF8 spelling accepted by Go paths.
    #[must_use]
    pub fn from_path(path: &Path) -> Self {
        Self::from_bytes(path.as_os_str().as_encoded_bytes())
    }
    /// Original bytes for human output and materialization.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Decoded Go text for text-only scans, never a filesystem spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.decoded
    }
    /// Number of original Go string bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }
    /// Whether the byte string is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
    /// Adds a valid text prefix while preserving all original bytes.
    #[must_use]
    pub fn prefixed(&self, prefix: &str) -> Self {
        let mut bytes = prefix.as_bytes().to_vec();
        bytes.extend_from_slice(&self.bytes);
        Self::from_bytes(&bytes)
    }
    /// Exact Go JSON string token, including HTML and JS-separator escaping.
    /// # Panics
    /// Only if a validated prefix or infallible string encoding invariant is violated.
    #[must_use]
    pub fn json_token(&self) -> String {
        let mut token = String::from("\"");
        let mut rest = self.bytes.as_slice();
        while !rest.is_empty() {
            let (valid, invalid) = match std::str::from_utf8(rest) {
                Ok(_) => (rest.len(), false),
                Err(error) => (error.valid_up_to(), true),
            };
            let encoded = serde_json::to_string(
                std::str::from_utf8(&rest[..valid]).expect("validated prefix"),
            )
            .expect("string serialization");
            token.push_str(
                &encoded[1..encoded.len() - 1]
                    .replace('&', "\\u0026")
                    .replace('<', "\\u003c")
                    .replace('>', "\\u003e")
                    .replace('\u{2028}', "\\u2028")
                    .replace('\u{2029}', "\\u2029"),
            );
            rest = &rest[valid..];
            if invalid {
                token.push_str("\\ufffd");
                rest = &rest[1..];
            }
        }
        token.push('"');
        token
    }
}
impl From<String> for GoText {
    fn from(value: String) -> Self {
        Self::from_bytes(value.as_bytes())
    }
}
impl From<&str> for GoText {
    fn from(value: &str) -> Self {
        Self::from_bytes(value.as_bytes())
    }
}
impl Deref for GoText {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}
impl fmt::Display for GoText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
impl PartialEq<String> for GoText {
    fn eq(&self, value: &String) -> bool {
        self.bytes == value.as_bytes()
    }
}
impl PartialEq<str> for GoText {
    fn eq(&self, value: &str) -> bool {
        self.bytes == value.as_bytes()
    }
}
impl PartialEq<&str> for GoText {
    fn eq(&self, value: &&str) -> bool {
        self == *value
    }
}
impl Serialize for GoText {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RawValue::from_string(self.json_token())
            .expect("valid Go JSON token")
            .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for GoText {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        String::deserialize(decoder).map(Self::from)
    }
}
/// Serialize a native path without losing bytes before Go JSON encoding.
/// # Errors
/// Returns the serializer error.
pub fn serialize_path<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
    GoText::from_path(path).serialize(serializer)
}

/// Serialize an optional native path through the same Go byte boundary.
/// # Errors
/// Returns the serializer error.
#[allow(clippy::ref_option)] // Serde field callbacks receive &Option<PathBuf>.
pub fn serialize_optional_path<S: Serializer>(
    path: &Option<std::path::PathBuf>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match path {
        Some(path) => serializer.serialize_some(&GoText::from_path(path)),
        None => serializer.serialize_none(),
    }
}
