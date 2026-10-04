//! Byte-valued Go diagnostics kept intact until JSON or human output.
use std::fmt;
use std::path::Path;

use serde::{Serialize, Serializer};

#[derive(Debug, Clone, Default)]
/// A Go diagnostic whose Unix path or subprocess bytes may not be UTF-8.
/// Human consumers use `AsRef<[u8]>`; Display is a conventional lossy view.
/// JSON serialization reproduces Go encoding/json, including HTML escaping.
pub struct GoText(Vec<u8>);

impl GoText {
    #[must_use]
    pub fn path(prefix: &str, path: &Path, suffix: &str) -> Self {
        let mut text = Self::from(prefix);
        text.push(&crate::config::os_bytes(path.as_os_str()));
        text.push(suffix.as_bytes());
        text
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    #[must_use]
    pub fn with_prefix(mut self, prefix: &str) -> Self {
        let mut bytes = prefix.as_bytes().to_vec();
        bytes.append(&mut self.0);
        Self(bytes)
    }

    #[must_use]
    pub fn with_suffix(mut self, suffix: &[u8]) -> Self {
        self.push(suffix);
        self
    }

    /// Repairs invalid bytes one byte at a time, as Go range/encoding does.
    /// Use only at Unicode metadata boundaries, never for filesystem selectors.
    #[must_use]
    pub fn unicode_lossy(&self) -> String {
        let mut result = String::new();
        let mut remaining = self.0.as_slice();
        while !remaining.is_empty() {
            match std::str::from_utf8(remaining) {
                Ok(valid) => {
                    result.push_str(valid);
                    break;
                }
                Err(error) => {
                    let prefix = error.valid_up_to();
                    if let Ok(valid) = std::str::from_utf8(&remaining[..prefix]) {
                        result.push_str(valid);
                    }
                    result.push('\u{fffd}');
                    remaining = &remaining[prefix + 1..];
                }
            }
        }
        result
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl AsRef<[u8]> for GoText {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<String> for GoText {
    fn from(text: String) -> Self {
        Self(text.into_bytes())
    }
}
impl From<&str> for GoText {
    fn from(text: &str) -> Self {
        Self(text.as_bytes().to_vec())
    }
}
impl From<Vec<u8>> for GoText {
    fn from(text: Vec<u8>) -> Self {
        Self(text)
    }
}

// Conventional Error/Display callers retain their text interface. Source
// reports and human writes use the raw bytes directly, never this projection.
impl fmt::Display for GoText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&String::from_utf8_lossy(&self.0))
    }
}

impl Serialize for GoText {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        crate::go_json_string_bytes(&self.0)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::GoText;

    #[test]
    fn json_escapes_html_javascript_and_controls_without_replacing_valid_text() {
        let message = GoText::from("<>&\u{2028}\u{2029}\n\t\"\\\u{fffd}");
        assert_eq!(
            serde_json::to_string(&message).unwrap(),
            "\"\\u003c\\u003e\\u0026\\u2028\\u2029\\n\\t\\\"\\\\\u{fffd}\""
        );
    }

    #[test]
    fn malformed_bytes_and_literal_replacement_character_remain_distinct() {
        let raw = b"path\xff\xe2\x82".to_vec();
        let message = GoText::from(raw.clone()).with_suffix(" literal\u{fffd}".as_bytes());
        assert_eq!(
            serde_json::to_string(&message).unwrap(),
            "\"path\\ufffd\\ufffd\\ufffd literal\u{fffd}\""
        );
        assert!(message.as_ref().starts_with(&raw));
    }
}
