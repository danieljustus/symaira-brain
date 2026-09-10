use serde::Serialize;
use serde_json::Value;
use serde_json::value::RawValue;
use symbrain_broker::CallToolResult;
use symbrain_catalog::Entry;
use symbrain_mcp::{ErrorObject, JSONRPC_VERSION};

/// Serializable JSON-RPC response retaining struct field order in results.
#[derive(Debug, Serialize)]
pub struct GatewayResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Box<RawValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

impl GatewayResponse {
    pub(crate) fn success<T: Serialize>(id: Value, value: T) -> Result<Self, crate::GatewayError> {
        let value = serde_json::to_string(&value)
            .map_err(|error| crate::GatewayError::Serialization(error.to_string()))?;
        let result = RawValue::from_string(value)
            .map_err(|error| crate::GatewayError::Serialization(error.to_string()))?;
        Ok(Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: Some(result),
            error: None,
        })
    }

    pub(crate) fn error(id: Value, code: i64, message: impl Into<String>) -> Self {
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

#[derive(Debug, Serialize)]
pub(crate) struct ToolsResult {
    pub(crate) tools: Vec<ListedTool>,
}

#[derive(Debug, Serialize)]
pub struct ListedTool {
    pub annotations: ToolAnnotations,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(rename = "inputSchema", skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Box<RawValue>>,
    pub name: String,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct ToolAnnotations {
    pub title: String,
    #[serde(rename = "readOnlyHint", skip_serializing_if = "is_false")]
    pub read_only_hint: bool,
    #[serde(rename = "destructiveHint", skip_serializing_if = "is_false")]
    pub destructive_hint: bool,
    #[serde(rename = "idempotentHint", skip_serializing_if = "is_false")]
    pub idempotent_hint: bool,
    #[serde(rename = "openWorldHint", skip_serializing_if = "is_false")]
    pub open_world_hint: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !value
}

impl ListedTool {
    pub(crate) fn from_entry(entry: &Entry) -> Self {
        let annotations = entry.tool.annotations.as_ref();
        Self {
            annotations: ToolAnnotations {
                title: annotations
                    .and_then(|value| value.title.clone())
                    .unwrap_or_else(|| entry.tool.name.clone()),
                read_only_hint: annotations
                    .and_then(|value| value.read_only_hint)
                    .unwrap_or(false),
                destructive_hint: annotations
                    .and_then(|value| value.destructive_hint)
                    .unwrap_or(false),
                idempotent_hint: annotations
                    .and_then(|value| value.idempotent_hint)
                    .unwrap_or(false),
                open_world_hint: annotations
                    .and_then(|value| value.open_world_hint)
                    .unwrap_or(false),
            },
            description: entry.tool.description.clone(),
            input_schema: entry.tool.input_schema.clone(),
            name: entry.tool.name.clone(),
        }
    }
}

pub(crate) fn usage_tool() -> ListedTool {
    ListedTool {
        annotations: ToolAnnotations {
            title: "AI Usage".to_string(),
            read_only_hint: true,
            destructive_hint: false,
            idempotent_hint: true,
            open_world_hint: false,
        },
        description: "Fetch AI subscription/token usage across providers (Claude, Codex, Copilot, Cursor, Kimi, Moonshot, Nous Portal, OpenCode, OpenRouter, Antigravity). Returns the schema-versioned usage report. Read-only.".to_string(),
        input_schema: RawValue::from_string(
            r#"{"type":"object","properties":{}}"#.to_string(),
        )
        .ok(),
        name: "get_ai_usage".to_string(),
    }
}

pub(crate) fn builtin_tool(name: &str) -> ListedTool {
    let (title, description) = if name == "bootstrap" {
        (
            "Bootstrap",
            "Call this first in every session. Returns the active profile's exposure summary (which cores and tool sets are available) and the live tool catalog (names only — vault values are never included).",
        )
    } else {
        (
            "Patterns",
            "List promoted patterns for this profile: tool-call sequences that recurred across multiple sessions, with their trigger conditions and provenance. Read-only — patterns are never executed by symbrain.",
        )
    };
    ListedTool {
        annotations: ToolAnnotations {
            title: title.to_string(),
            read_only_hint: true,
            destructive_hint: false,
            idempotent_hint: true,
            open_world_hint: false,
        },
        description: description.to_string(),
        input_schema: RawValue::from_string(r#"{"properties":{},"type":"object"}"#.to_string())
            .ok(),
        name: name.to_string(),
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ToolResultWire {
    pub(crate) content: Vec<ContentWire>,
    #[serde(rename = "isError")]
    pub(crate) is_error: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ContentWire {
    pub(crate) text: String,
    #[serde(rename = "type")]
    pub(crate) kind: String,
}

impl ToolResultWire {
    pub(crate) fn from_result(result: &CallToolResult) -> Self {
        Self {
            content: result
                .content
                .iter()
                .map(|block| ContentWire {
                    text: block.text.clone(),
                    kind: block.kind.clone(),
                })
                .collect(),
            is_error: result.is_error,
        }
    }
}

pub(crate) fn tool_result(text: String, is_error: bool) -> ToolResultWire {
    ToolResultWire {
        content: vec![ContentWire {
            text,
            kind: "text".to_string(),
        }],
        is_error,
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;
    use serde::ser::{Error as _, Serializer};

    use super::*;

    struct FailingPayload;

    impl Serialize for FailingPayload {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            Err(S::Error::custom("intentional serialization failure"))
        }
    }

    #[test]
    fn success_response_never_swallows_serialization_errors() {
        let error = GatewayResponse::success(Value::from(1), FailingPayload).unwrap_err();
        assert!(matches!(error, crate::GatewayError::Serialization(_)));
        assert!(
            error
                .to_string()
                .contains("intentional serialization failure")
        );
    }
}
