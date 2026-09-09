use std::collections::BTreeMap;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use serde_json::value::RawValue;

pub const JSONRPC_VERSION: &str = "2.0";
pub const PROTOCOL_VERSION: &str = "2024-11-05";
pub const CODE_PARSE_ERROR: i64 = -32700;
pub const CODE_INVALID_REQUEST: i64 = -32600;
pub const CODE_METHOD_NOT_FOUND: i64 = -32601;
pub const CODE_INVALID_PARAMS: i64 = -32602;
pub const CODE_INTERNAL_ERROR: i64 = -32603;

/// Incoming JSON-RPC request, preserving the distinction between absent and null IDs.
#[derive(Debug, Clone)]
pub struct Request {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Box<RawValue>>,
    pub has_id: bool,
}

impl<'de> Deserialize<'de> for Request {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        if raw.get().trim() == "null" {
            return Ok(Self {
                jsonrpc: String::new(),
                id: None,
                method: String::new(),
                params: None,
                has_id: false,
            });
        }
        let mut fields = serde_json::from_str::<BTreeMap<String, Box<RawValue>>>(raw.get())
            .map_err(D::Error::custom)?;
        let has_id = fields.contains_key("id");
        let id = fields
            .remove("id")
            .map(|value| serde_json::from_str::<Value>(value.get()))
            .transpose()
            .map_err(D::Error::custom)?
            .filter(|value| !value.is_null());
        let jsonrpc =
            decode_string(fields.remove("jsonrpc"), "jsonrpc").map_err(D::Error::custom)?;
        let method = decode_string(fields.remove("method"), "method").map_err(D::Error::custom)?;
        let params = fields.remove("params");
        Ok(Self {
            jsonrpc,
            id,
            method,
            params,
            has_id,
        })
    }
}

fn decode_string(value: Option<Box<RawValue>>, field: &str) -> Result<String, String> {
    value.map_or_else(
        || Ok(String::new()),
        |value| {
            serde_json::from_str(value.get()).map_err(|error| format!("invalid {field}: {error}"))
        },
    )
}

impl Request {
    #[must_use]
    pub fn is_notification(&self) -> bool {
        !self.has_id && self.id.is_none()
    }
}

/// Outgoing JSON-RPC request or notification.
#[derive(Debug, Clone, Serialize)]
pub struct RequestMessage<'a> {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<&'a RawValue>,
}

impl<'a> RequestMessage<'a> {
    #[must_use]
    pub const fn new(id: Option<i64>, method: &'a str, params: Option<&'a RawValue>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION,
            id,
            method,
            params,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorObject {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JSON-RPC response with exact null/omission semantics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

impl Response {
    #[must_use]
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    #[must_use]
    pub fn error(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(ErrorObject {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
    pub capabilities: BTreeMap<String, Value>,
    pub client_info: ClientInfo,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_distinguishes_absent_null_and_numeric_ids() {
        let absent: Request = serde_json::from_str(r#"{"jsonrpc":"2.0","method":"ping"}"#).unwrap();
        let null: Request =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#).unwrap();
        let numeric: Request =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#).unwrap();
        assert!(absent.is_notification());
        assert!(!null.is_notification());
        assert_eq!(numeric.id, Some(Value::from(1)));
    }

    #[test]
    fn response_omits_opposite_branch_and_keeps_null_id() {
        assert_eq!(
            serde_json::to_string(&Response::error(
                Value::Null,
                CODE_PARSE_ERROR,
                "Parse error"
            ))
            .unwrap(),
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}"#
        );
    }
}
