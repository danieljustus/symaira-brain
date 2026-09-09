//! Pure request/decision models and fail-closed validation for Guard.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::missing_errors_doc,
    clippy::too_many_lines,
    clippy::trivially_copy_pass_by_ref
)]

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// Version of the Guard action-event model.
pub const SCHEMA_VERSION: i32 = 1;

/// A source of a policy-relevant event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    Proxy,
    Hook,
    Artifact,
    Scan,
    Decide,
}

impl SourceType {
    /// Validates an untyped wire value without silently accepting a new value.
    pub fn validate(value: &str) -> Result<Self, ModelError> {
        match value {
            "proxy" => Ok(Self::Proxy),
            "hook" => Ok(Self::Hook),
            "artifact" => Ok(Self::Artifact),
            "scan" => Ok(Self::Scan),
            "decide" => Ok(Self::Decide),
            other => Err(ModelError::new(format!(
                "model: unknown source type {other:?}"
            ))),
        }
    }
}

impl fmt::Display for SourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Proxy => "proxy",
            Self::Hook => "hook",
            Self::Artifact => "artifact",
            Self::Scan => "scan",
            Self::Decide => "decide",
        };
        f.write_str(value)
    }
}

/// Lifecycle state of an action event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionState {
    Requested,
    Approved,
    Denied,
    Started,
    Completed,
    Failed,
}

impl ActionState {
    /// Validates an untyped wire value.
    pub fn validate(value: &str) -> Result<Self, ModelError> {
        match value {
            "requested" => Ok(Self::Requested),
            "approved" => Ok(Self::Approved),
            "denied" => Ok(Self::Denied),
            "started" => Ok(Self::Started),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            other => Err(ModelError::new(format!(
                "model: unknown action state {other:?}"
            ))),
        }
    }
}

/// The policy decision returned to the runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Allow,
    Ask,
    Deny,
    Require,
    Redact,
    #[serde(rename = "readonly")]
    ReadOnly,
    Sandbox,
}

impl Decision {
    /// Validates an untyped wire value.
    pub fn validate(value: &str) -> Result<Self, ModelError> {
        match value {
            "allow" => Ok(Self::Allow),
            "ask" => Ok(Self::Ask),
            "deny" => Ok(Self::Deny),
            "require" => Ok(Self::Require),
            "redact" => Ok(Self::Redact),
            "readonly" => Ok(Self::ReadOnly),
            "sandbox" => Ok(Self::Sandbox),
            other => Err(ModelError::new(format!(
                "model: unknown decision {other:?}"
            ))),
        }
    }
}

/// How a failed external decision is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureMode {
    Deny,
    Allow,
    /// Preserves an unrecognized wire value so callers can fail closed like Go.
    Unknown(String),
}

impl Serialize for FailureMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(match self {
            Self::Deny => "deny",
            Self::Allow => "allow",
            Self::Unknown(value) => value,
        })
    }
}

impl<'de> Deserialize<'de> for FailureMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "deny" => Self::Deny,
            "allow" => Self::Allow,
            _ => Self::Unknown(value),
        })
    }
}

impl FailureMode {
    /// Resolves the mode. Unknown and absent values fail closed to deny.
    #[must_use]
    pub const fn resolve(&self) -> Decision {
        match self {
            Self::Allow => Decision::Allow,
            Self::Deny | Self::Unknown(_) => Decision::Deny,
        }
    }

    /// Returns the exact wire spelling for validation and diagnostics.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Deny => "deny",
            Self::Allow => "allow",
            Self::Unknown(value) => value,
        }
    }

    /// Validates an optional wire value; absence and empty are fail-closed defaults.
    pub fn validate(value: Option<&str>) -> Result<(), ModelError> {
        match value {
            Some("" | "deny" | "allow") | None => Ok(()),
            Some(other) => Err(ModelError::new(format!(
                "model: unknown failure mode {other:?}"
            ))),
        }
    }
}

/// Agent identity attached to an event.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

/// Client identity attached to an event.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientIdentity {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

/// An MCP tool call under evaluation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolCall {
    pub server: String,
    pub tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A request to produce a decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRequest {
    pub call: ToolCall,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_mode: Option<FailureMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline: Option<DateTime<FixedOffset>>,
}

/// A safe control-plane response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlResponse {
    pub decision: Decision,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "is_zero")]
    pub retry_after: i32,
}

fn is_zero(value: &i32) -> bool {
    *value == 0
}

/// The non-decision result used by every transport/error path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoDecision {
    pub control: ControlResponse,
    pub diagnostic: String,
}

/// Policy diagnostic information, never required by the control path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evaluation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_rule: Option<String>,
    pub decision: Decision,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub marginal_check: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rule_trace: Vec<RuleTraceEntry>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// One diagnostic rule-trace entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleTraceEntry {
    pub rule_id: String,
    pub matched: bool,
    pub decision: Decision,
    pub bucket: String,
}

/// Immutable action event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionEvent {
    pub id: String,
    pub schema_version: i32,
    pub source: SourceType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_event_id: Option<String>,
    pub agent: AgentIdentity,
    pub client: ClientIdentity,
    pub call: ToolCall,
    pub state: ActionState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluation: Option<Evaluation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control_response: Option<ControlResponse>,
    pub timestamp: String,
}

/// An actionable model validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelError(String);

impl ModelError {
    pub(crate) fn new(message: String) -> Self {
        Self(message)
    }
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ModelError {}

/// Returns the stable event ID format used by Go.
#[must_use]
pub fn event_id(source: &SourceType, counter: i64) -> String {
    format!("evt_{SCHEMA_VERSION}_{source}_{counter}")
}

/// Validates a tool call's required identity fields.
pub fn validate_tool_call(call: &ToolCall) -> Result<(), ModelError> {
    if call.server.trim().is_empty() {
        return Err(ModelError::new("model: call server is required".into()));
    }
    if call.tool.trim().is_empty() {
        return Err(ModelError::new("model: call tool is required".into()));
    }
    Ok(())
}

/// Validates a decision request without evaluating its policy.
pub fn validate_request(request: &DecisionRequest) -> Result<(), ModelError> {
    validate_tool_call(&request.call)
        .map_err(|e| ModelError::new(format!("model: invalid decision request: {e}")))?;
    let failure = request.failure_mode.as_ref().map(FailureMode::as_str);
    FailureMode::validate(failure)
        .map_err(|e| ModelError::new(format!("model: invalid decision request: {e}")))
}

/// Validates a control response.
pub fn validate_control(response: &ControlResponse) -> Result<(), ModelError> {
    if response.retry_after < 0 {
        return Err(ModelError::new(
            "model: invalid control response: retry_after must not be negative".into(),
        ));
    }
    Ok(())
}

/// Returns true at or after a non-zero RFC3339 deadline.
pub fn expired_at(deadline: Option<&str>, now: &str) -> Result<bool, ModelError> {
    let Some(deadline) = deadline.filter(|value| !value.is_empty()) else {
        return Ok(false);
    };
    let deadline = parse_time(deadline)?;
    let now = parse_time(now)?;
    Ok(now >= deadline)
}

fn parse_time(value: &str) -> Result<DateTime<FixedOffset>, ModelError> {
    DateTime::parse_from_rfc3339(value)
        .map_err(|error| ModelError::new(format!("model: invalid timestamp {value:?}: {error}")))
}

/// Creates the single fail-closed non-decision shape.
#[must_use]
pub fn new_no_decision(failure: Option<&FailureMode>, diagnostic: impl Into<String>) -> NoDecision {
    let decision = failure.map_or(Decision::Deny, FailureMode::resolve);
    NoDecision {
        control: ControlResponse {
            decision,
            reason: None,
            retry_after: 0,
        },
        diagnostic: diagnostic.into(),
    }
}
