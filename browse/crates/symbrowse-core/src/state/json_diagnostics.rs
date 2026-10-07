//! Go's invalid-first-byte diagnostic around Serde; acceptance stays unchanged.
use super::StateError;

pub(super) fn payload_error(payload: &[u8], error: serde_json::Error) -> StateError {
    let message = if error.to_string().starts_with("expected value") {
        invalid_first_byte(payload).unwrap_or_else(|| error.to_string())
    } else {
        error.to_string()
    };
    StateError::InvalidPayload(message)
}

pub(crate) fn invalid_first_byte(payload: &[u8]) -> Option<String> {
    // JSON whitespace is exactly these four bytes, not ASCII/Unicode space.
    let first = *payload
        .iter()
        .find(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))?;
    if matches!(
        first,
        b'{' | b'[' | b'"' | b'-' | b'0'..=b'9' | b't' | b'f' | b'n'
    ) {
        return None;
    }
    let token = if first == b'\'' {
        "\\'".to_owned()
    } else {
        // Go's scanner quotes string(byte), i.e. its Latin-1 scalar value.
        let quoted = crate::go_quote::quote(&char::from(first).to_string());
        quoted[1..quoted.len() - 1].to_owned()
    };
    Some(format!(
        "invalid character '{token}' looking for beginning of value"
    ))
}
