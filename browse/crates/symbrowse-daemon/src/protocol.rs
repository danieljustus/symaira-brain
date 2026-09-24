use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// One newline-delimited request to the daemon.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct Frame {
    pub cmd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub session: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub retrieval_surface: String,
}

/// A stable daemon error payload. Secret-bearing text must be redacted before
/// it is constructed by an application handler.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct DaemonError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_user_confirmation: Option<bool>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub resume_hint: String,
}

impl fmt::Display for DaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for DaemonError {}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct Warning {
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub r#ref: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub excerpt: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Response {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DaemonError>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<Warning>,
}

pub mod codes {
    pub const MALFORMED_REQUEST: &str = "malformed_request";
    pub const UNKNOWN_COMMAND: &str = "unknown_command";
    pub const OPERATION_TIMEOUT: &str = "operation_timeout";
    pub const OPERATION_FAILED: &str = "operation_failed";
    pub const PEER_DENIED: &str = "peer_denied";
    pub const DAEMON_UNAVAILABLE: &str = "daemon_unavailable";
    pub const INVALID_SESSION: &str = "invalid_session";
    pub const SESSION_NOT_FOUND: &str = "session_not_found";
    pub const SESSION_USER_CONTROL: &str = "session_user_control";
    pub const SESSION_INACTIVE: &str = "session_inactive";
    pub const HANDOFF_TIMEOUT: &str = "handoff_timeout";
}
pub use codes as ErrorCode;

pub fn decode_frame(raw: &[u8]) -> Result<Frame, DaemonError> {
    if raw.len() > crate::MAX_FRAME_BYTES {
        return Err(DaemonError {
            code: codes::MALFORMED_REQUEST.into(),
            message: "daemon frame exceeds size limit".into(),
            ..Default::default()
        });
    }
    let frame: Frame = serde_json::from_slice(raw).map_err(|error| DaemonError {
        code: codes::MALFORMED_REQUEST.into(),
        message: format!("decode frame: {}", go_json_error(raw, &error)),
        ..Default::default()
    })?;
    if frame.cmd.is_empty() {
        return Err(DaemonError {
            code: codes::MALFORMED_REQUEST.into(),
            message: "missing cmd".into(),
            ..Default::default()
        });
    }
    Ok(frame)
}

fn go_json_error(raw: &[u8], error: &serde_json::Error) -> String {
    let detail = error.to_string();
    if error.classify() == serde_json::error::Category::Eof {
        return "unexpected end of JSON input".into();
    }

    let offset = raw
        .split_inclusive(|byte| *byte == b'\n')
        .take(error.line().saturating_sub(1))
        .map(<[u8]>::len)
        .sum::<usize>()
        .saturating_add(error.column().saturating_sub(1));
    let character = raw.get(offset).copied().unwrap_or_default();
    if detail.starts_with("invalid escape") {
        return format!(
            "invalid character {} in string escape code",
            go_quote_byte(character)
        );
    }
    if detail.starts_with("invalid number") {
        let previous = raw.get(offset.saturating_sub(1)).copied();
        let context = match previous {
            Some(b'e' | b'E') => "in exponent of numeric literal",
            Some(b'0') if character.is_ascii_digit() => "after object key:value pair",
            _ => return detail,
        };
        return format!("invalid character {} {context}", go_quote_byte(character));
    }

    let context = if detail.starts_with("key must be a string") {
        "looking for beginning of object key string"
    } else if detail.starts_with("trailing characters") {
        "after top-level value"
    } else {
        return detail;
    };

    // Go's encoding/json reports the offending byte and parser state here,
    // while serde_json reports a category and line/column. Keep this mapping
    // at the shared frame boundary so malformed daemon requests stay stable.
    format!("invalid character {} {context}", go_quote_byte(character))
}

fn go_quote_byte(byte: u8) -> String {
    match byte {
        b'\'' => "'\\''".to_owned(),
        b'"' => "'\"'".to_owned(),
        b'\\' => "'\\\\'".to_owned(),
        b'\x07' => "'\\a'".to_owned(),
        b'\x08' => "'\\b'".to_owned(),
        b'\x0c' => "'\\f'".to_owned(),
        b'\n' => "'\\n'".to_owned(),
        b'\r' => "'\\r'".to_owned(),
        b'\t' => "'\\t'".to_owned(),
        b'\x0b' => "'\\v'".to_owned(),
        0x20..=0x7e => format!("'{}'", char::from(byte)),
        _ => format!("'\\x{byte:02x}'"),
    }
}

pub fn error_response(code: impl Into<String>, message: impl Into<String>) -> Response {
    Response {
        success: false,
        data: None,
        error: Some(DaemonError {
            code: code.into(),
            message: message.into(),
            ..Default::default()
        }),
        warnings: Vec::new(),
    }
}

pub fn success_response(data: Option<Value>, warnings: Vec<Warning>) -> Response {
    Response {
        success: true,
        data,
        error: None,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_and_response_omit_empty_optional_fields() {
        let raw = serde_json::to_string(&Frame {
            cmd: "daemon.status".into(),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(raw, r#"{"cmd":"daemon.status"}"#);
        let raw = serde_json::to_string(&success_response(
            Some(serde_json::json!({"running":true})),
            Vec::new(),
        ))
        .unwrap();
        assert_eq!(raw, r#"{"success":true,"data":{"running":true}}"#);
    }
    #[test]
    fn empty_command_and_one_mib_boundary_are_checked() {
        assert_eq!(
            decode_frame(br#"{"session":"x"}"#).unwrap_err().code,
            codes::MALFORMED_REQUEST
        );
        assert_eq!(
            decode_frame(br#"{"cmd":" "}"#).unwrap().cmd,
            " ",
            "Go rejects only an empty command; whitespace reaches command dispatch"
        );
        assert_eq!(
            decode_frame(b"{\"cmd\":\"x\"}\x0b").unwrap_err().code,
            codes::MALFORMED_REQUEST
        );
        assert_eq!(
            decode_frame(b"{not json").unwrap_err().message,
            "decode frame: invalid character 'n' looking for beginning of object key string"
        );
        assert_eq!(
            decode_frame(b"{\"cmd\":\"x\"}\x0b").unwrap_err().message,
            "decode frame: invalid character '\\v' after top-level value"
        );
        let value = serde_json::json!({"cmd":"x","args": "x".repeat(crate::MAX_FRAME_BYTES)});
        let raw = serde_json::to_vec(&value).unwrap();
        assert!(raw.len() > crate::MAX_FRAME_BYTES);
        assert_eq!(
            decode_frame(&raw).unwrap_err().code,
            codes::MALFORMED_REQUEST
        );
    }

    #[test]
    fn malformed_json_messages_match_go() {
        let cases: &[(&[u8], &str)] = &[
            (b"{\"cmd\":", "decode frame: unexpected end of JSON input"),
            (
                b"{\"cmd\":\"x\"",
                "decode frame: unexpected end of JSON input",
            ),
            (
                b"{\"cmd\":\"x\\q\"}",
                "decode frame: invalid character 'q' in string escape code",
            ),
            (
                b"{\"cmd\":01}",
                "decode frame: invalid character '1' after object key:value pair",
            ),
            (
                b"{\"cmd\":1e}",
                "decode frame: invalid character '}' in exponent of numeric literal",
            ),
        ];
        for (raw, expected) in cases {
            assert_eq!(decode_frame(raw).unwrap_err().message, *expected, "{raw:?}");
        }
    }
}
