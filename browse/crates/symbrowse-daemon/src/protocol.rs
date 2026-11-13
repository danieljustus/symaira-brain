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
    if let Some(kind) = scalar_or_array_kind(raw) {
        // Go decodes `null` into a struct as a no-op and rejects every other
        // non-object value; serde would also accept an array as a field tuple.
        let message = if kind == "null" {
            "missing cmd".to_owned()
        } else {
            format!(
                "decode frame: json: cannot unmarshal {kind} into Go value of type daemon.Frame"
            )
        };
        return Err(DaemonError {
            code: codes::MALFORMED_REQUEST.into(),
            message,
            ..Default::default()
        });
    }
    let frame: Frame = serde_json::from_slice(raw).map_err(|e| DaemonError {
        code: codes::MALFORMED_REQUEST.into(),
        message: format!("decode frame: {e}"),
        ..Default::default()
    })?;
    // Go compares `frame.Cmd == ""`: a whitespace-only name is a command.
    if frame.cmd.is_empty() {
        return Err(DaemonError {
            code: codes::MALFORMED_REQUEST.into(),
            message: "missing cmd".into(),
            ..Default::default()
        });
    }
    Ok(frame)
}

/// Go's JSON type name of a complete top-level non-object value, if `raw` is one.
fn scalar_or_array_kind(raw: &[u8]) -> Option<&'static str> {
    // JSON whitespace is exactly these four bytes.
    let first = raw
        .iter()
        .find(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))?;
    let kind = match first {
        b'[' => "array",
        b'"' => "string",
        b't' | b'f' => "bool",
        b'-' | b'0'..=b'9' => "number",
        b'n' => "null",
        _ => return None,
    };
    serde_json::from_slice::<serde::de::IgnoredAny>(raw)
        .is_ok()
        .then_some(kind)
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
    fn only_json_objects_are_frames() {
        for (raw, message) in [
            (
                &br#"["daemon.ping"]"#[..],
                "decode frame: json: cannot unmarshal array into Go value of type daemon.Frame",
            ),
            (
                br#" "daemon.ping""#,
                "decode frame: json: cannot unmarshal string into Go value of type daemon.Frame",
            ),
            (
                b"true",
                "decode frame: json: cannot unmarshal bool into Go value of type daemon.Frame",
            ),
            (
                b"-1.5e3",
                "decode frame: json: cannot unmarshal number into Go value of type daemon.Frame",
            ),
            (b"null", "missing cmd"),
        ] {
            let error = decode_frame(raw).unwrap_err();
            assert_eq!(error.code, codes::MALFORMED_REQUEST, "{raw:?}");
            assert_eq!(error.message, message, "{raw:?}");
        }
        // Incomplete values stay syntax errors and objects are unaffected.
        assert!(
            decode_frame(b"[")
                .unwrap_err()
                .message
                .starts_with("decode frame: ")
        );
        assert_eq!(decode_frame(br#"{"cmd":" "}"#).unwrap().cmd, " ");
    }
    #[test]
    fn empty_command_and_one_mib_boundary_are_checked() {
        assert_eq!(
            decode_frame(br#"{"session":"x"}"#).unwrap_err().code,
            codes::MALFORMED_REQUEST
        );
        let value = serde_json::json!({"cmd":"x","args": "x".repeat(crate::MAX_FRAME_BYTES)});
        let raw = serde_json::to_vec(&value).unwrap();
        assert!(raw.len() > crate::MAX_FRAME_BYTES);
        assert_eq!(
            decode_frame(&raw).unwrap_err().code,
            codes::MALFORMED_REQUEST
        );
    }
}
