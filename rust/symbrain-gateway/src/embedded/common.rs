use serde_json::Value;
use symbrain_mcp::DispatchContext;

use crate::GatewayError;
use crate::response::{GatewayResponse, tool_result};
use symbrain_memory::Store;

pub(crate) fn arg<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key)
}

pub(crate) fn string_arg(value: &Value, key: &str) -> Result<String, GatewayError> {
    arg(value, key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| GatewayError::InvalidArguments(format!("'{key}' is required")))
}

pub(crate) fn limit(value: &Value, default: usize, max: usize) -> usize {
    value
        .get("limit")
        .and_then(Value::as_u64)
        .and_then(|number| usize::try_from(number).ok())
        .unwrap_or(default)
        .min(max)
        .max(1)
}

pub(crate) fn usize_value(value: &Value, key: &str, default: usize) -> usize {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|number| usize::try_from(number).ok())
        .unwrap_or(default)
}

pub(crate) fn text_response(
    id: Value,
    text: String,
    error: bool,
) -> Result<GatewayResponse, GatewayError> {
    GatewayResponse::success(id, tool_result(text, error))
}

pub(crate) fn pretty<T: serde::Serialize>(value: &T) -> Result<String, GatewayError> {
    serde_json::to_string_pretty(value)
        .map_err(|error| GatewayError::Serialization(error.to_string()))
}

pub(crate) fn check_cancel(context: DispatchContext<'_>) -> Result<(), GatewayError> {
    if context.is_cancelled() {
        Err(GatewayError::Cancelled)
    } else {
        Ok(())
    }
}

pub(crate) fn dispatch_memory(
    store: &Store,
    name: &str,
    value: &Value,
) -> Result<String, GatewayError> {
    super::memory::dispatch(store, name, value)
}

pub(crate) fn dispatch_activity(
    store: &Store,
    name: &str,
    value: &Value,
) -> Result<String, GatewayError> {
    super::activity::dispatch(store, name, value)
}
