#![deny(unsafe_code)]

//! Owned Firefox WebDriver BiDi adapter.  Firefox is launched with an isolated
//! profile and a loopback-only BiDi socket; no geckodriver or Chrome fallback
//! is involved.

use serde_json::{Value, json};
mod error;
mod executable;
mod network;
mod session;
mod startup;
pub use error::FirefoxError;
pub use executable::resolve_firefox_executable;
use symbrowse_engine::{
    EvaluationResult,
    capabilities::{Capabilities, capabilities_for},
};

pub const ENGINE_KIND: &str = "firefox";

// BiDi represents object properties as [key, RemoteValue] pairs.
fn decode_remote_value(remote: &Value) -> Value {
    match remote.get("type").and_then(Value::as_str) {
        Some("undefined" | "null") => Value::Null,
        Some("object") => {
            let Some(properties) = remote.get("value").and_then(Value::as_array) else {
                return Value::Null;
            };
            let mut object = serde_json::Map::new();
            for property in properties {
                let Some(pair) = property.as_array() else {
                    continue;
                };
                let (Some(key), Some(value)) = (pair.first().and_then(Value::as_str), pair.get(1))
                else {
                    continue;
                };
                object.insert(key.to_owned(), decode_remote_value(value));
            }
            Value::Object(object)
        }
        Some("array") => remote
            .get("value")
            .and_then(Value::as_array)
            .map(|items| items.iter().map(decode_remote_value).collect())
            .map(Value::Array)
            .unwrap_or(Value::Null),
        Some(_) => remote.get("value").cloned().unwrap_or(Value::Null),
        None => Value::Null,
    }
}

fn evaluation_result(result: Value) -> EvaluationResult {
    if result.get("type").and_then(Value::as_str) == Some("exception") {
        return EvaluationResult {
            exception_text: result
                .get("exceptionDetails")
                .and_then(|details| details.get("text"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            ..EvaluationResult::default()
        };
    }
    let remote = result.get("result").unwrap_or(&Value::Null);
    EvaluationResult {
        value: Some(decode_remote_value(remote)),
        value_type: remote
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .into(),
        description: remote
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .into(),
        exception_text: String::new(),
    }
}

fn context_partition(context: &str) -> Value {
    // BiDi's partition descriptor is tagged; Firefox rejects a context-only
    // object even when the browsing-context id itself is valid.
    json!({"type":"context","context":context})
}

pub fn canonical_capabilities() -> Capabilities {
    capabilities_for(
        ENGINE_KIND,
        [
            "CookieEngine",
            "FrameManager",
            "InspectionEngine",
            "InteractionEngine",
            "NavigationStateProvider",
            "ScreenshotEngine",
            "TabManager",
        ],
    )
}

pub use session::FirefoxSession;

#[cfg(test)]
mod tests;
