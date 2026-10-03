//! Public Go store diagnostics around the existing codec; no crypto changes.
use super::{Store, StoreError, files};
use crate::state::{FILE_MAGIC, StateError};

pub(super) fn decode_error(name: &str, raw: &[u8], keyed: bool, error: StateError) -> StoreError {
    let message = match error {
        StateError::Decrypt => {
            "decrypt state file: cipher: message authentication failed".to_owned()
        }
        StateError::KeyRequired if !keyed => {
            legacy_json_error(raw).unwrap_or_else(|| StateError::KeyRequired.to_string())
        }
        StateError::InvalidPayload(ref message)
            if !keyed && message.starts_with("expected value") =>
        {
            legacy_json_error(raw).unwrap_or_else(|| error.to_string())
        }
        _ => error.to_string(),
    };
    StoreError::Context(format!(
        "decode state {}: {message}",
        crate::go_quote::quote(name)
    ))
}

pub(super) fn header_error(name: &str, raw: &[u8], keyed: bool, error: StateError) -> StoreError {
    match error {
        StateError::HeaderAuthentication(error) => {
            let message = match *error {
                StateError::Decrypt => {
                    "decrypt state file: cipher: message authentication failed".to_owned()
                }
                error => error.to_string(),
            };
            StoreError::Context(format!("authenticate state header: {message}"))
        }
        StateError::KeyRequired => StoreError::State(StateError::KeyRequired),
        error => decode_error(name, raw, keyed, error),
    }
}

fn legacy_json_error(raw: &[u8]) -> Option<String> {
    let data = raw.strip_prefix(FILE_MAGIC)?;
    let body = if let Some(newline) = data.iter().position(|byte| *byte == b'\n') {
        if let Ok(header) = serde_json::from_slice::<serde_json::Value>(&data[..newline]) {
            if header
                .get("schema_version")
                .and_then(serde_json::Value::as_u64)?
                != 2
            {
                return None;
            }
            &data[newline + 1..]
        } else {
            data
        }
    } else {
        data
    };
    crate::state::invalid_first_byte(body).map(|message| format!("parse state payload: {message}"))
}

impl Store {
    /// Warning-only untrusted metadata, never authorization or decryption proof.
    /// Reads a bounded prefix through the store's regular/no-follow file guard.
    #[must_use]
    pub fn existing_encrypted_key_source(&self, name: &str) -> Option<String> {
        crate::state::validate_name(name).ok()?;
        let raw = files::read_prefix(&self.path(name), 64 << 10).ok()?;
        let data = raw.strip_prefix(FILE_MAGIC)?;
        let newline = data.iter().position(|byte| *byte == b'\n')?;
        let header: serde_json::Value = serde_json::from_slice(&data[..newline]).ok()?;
        if header.get("schema_version")?.as_u64()? < 2 {
            return None;
        }
        let source = header.get("key_source")?.as_str()?;
        (!source.is_empty() && source != "none").then(|| source.to_owned())
    }
}
