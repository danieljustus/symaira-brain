//! Validation of complete Guard action-event wire values.

use crate::model::{
    ActionEvent, ActionState, AgentIdentity, Decision, ModelError, SCHEMA_VERSION, ToolCall,
    validate_tool_call,
};
use serde::Deserialize;
use serde_json::Value;

/// Validates a typed action event in the same order as the Go model oracle.
/// Enum fields are already validated by Rust's typed representation.
///
/// # Errors
/// Returns the first validation error in Go-compatible field order.
pub fn validate_action_event(event: &ActionEvent) -> Result<(), ModelError> {
    if event.id.trim().is_empty() {
        return Err(ModelError::new("model: event ID is required".into()));
    }
    if event.schema_version != SCHEMA_VERSION {
        return Err(ModelError::new(format!(
            "model: unsupported schema version {}",
            event.schema_version
        )));
    }
    if event.timestamp.trim().is_empty() {
        return Err(ModelError::new("model: event timestamp is required".into()));
    }
    if event.agent.agent_id.trim().is_empty() {
        return Err(ModelError::new("model: event agent ID is required".into()));
    }
    validate_call(&event.call)?;
    if let Some(control) = event.control_response.as_ref()
        && control.retry_after < 0
    {
        return Err(ModelError::new(
            "model: invalid control response: retry_after must not be negative".into(),
        ));
    }
    Ok(())
}

/// Validates a JSON action event before enum decoding, preserving the Go
/// oracle's stable order and messages for unknown source/state/decision values.
///
/// # Errors
/// Returns decoding or first-field validation errors in Go-compatible order.
pub fn validate_action_event_json(value: &Value) -> Result<(), ModelError> {
    let event: WireActionEvent = serde_json::from_value(value.clone())
        .map_err(|error| ModelError::new(format!("model: decode action event: {error}")))?;
    validate_identity_fields(
        &event.id,
        event.schema_version,
        &event.source,
        &event.state,
        &event.timestamp,
        &event.agent,
    )?;
    validate_call(&event.call)?;
    if let Some(control) = event.control_response.as_ref() {
        validate_control_values(&control.decision, control.retry_after)?;
    }
    if let Some(evaluation) = event.evaluation.as_ref() {
        validate_decision_value(&evaluation.decision, "model: invalid evaluation: ")?;
    }
    Ok(())
}

fn validate_identity_fields(
    id: &str,
    schema_version: i32,
    source: &str,
    state: &str,
    timestamp: &str,
    agent: &AgentIdentity,
) -> Result<(), ModelError> {
    if id.trim().is_empty() {
        return Err(ModelError::new("model: event ID is required".into()));
    }
    if schema_version != SCHEMA_VERSION {
        return Err(ModelError::new(format!(
            "model: unsupported schema version {schema_version}"
        )));
    }
    ActionEventSource::validate(source)?;
    ActionEventState::validate(state)?;
    if timestamp.trim().is_empty() {
        return Err(ModelError::new("model: event timestamp is required".into()));
    }
    if agent.agent_id.trim().is_empty() {
        return Err(ModelError::new("model: event agent ID is required".into()));
    }
    Ok(())
}

fn validate_call(call: &ToolCall) -> Result<(), ModelError> {
    validate_tool_call(call)
        .map_err(|error| ModelError::new(format!("model: invalid event call: {error}")))
}

fn validate_control_values(decision: &str, retry_after: i32) -> Result<(), ModelError> {
    validate_decision_value(decision, "model: invalid control response: ")?;
    if retry_after < 0 {
        return Err(ModelError::new(
            "model: invalid control response: retry_after must not be negative".into(),
        ));
    }
    Ok(())
}

fn validate_decision_value(value: &str, prefix: &str) -> Result<(), ModelError> {
    Decision::validate(value)
        .map(|_| ())
        .map_err(|error| ModelError::new(format!("{prefix}{error}")))
}

struct ActionEventSource;
impl ActionEventSource {
    fn validate(value: &str) -> Result<(), ModelError> {
        crate::model::SourceType::validate(value).map(|_| ())
    }
}

struct ActionEventState;
impl ActionEventState {
    fn validate(value: &str) -> Result<(), ModelError> {
        ActionState::validate(value).map(|_| ())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireActionEvent {
    id: String,
    schema_version: i32,
    source: String,
    agent: AgentIdentity,
    call: ToolCall,
    state: String,
    evaluation: Option<WireEvaluation>,
    control_response: Option<WireControlResponse>,
    timestamp: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireEvaluation {
    decision: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireControlResponse {
    decision: String,
    retry_after: i32,
}
