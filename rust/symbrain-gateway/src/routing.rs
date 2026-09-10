//! Namespaced routing and profile identity injection.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use serde_json::value::RawValue;
use serde_json::{Map, Value};
use symbrain_broker::CallToolResult;
use symbrain_catalog::{Catalog, Entry};
use symbrain_mcp::DispatchContext;
use symbrain_policy::{Profile, identity_parameter};

use crate::{GatewayBackend, GatewayError};

/// Forwards one exposed namespaced tool to its owning backend.
///
/// The child receives its original name. Memory calls receive the active
/// profile as `client_id` unless the caller supplied that key.
///
/// # Errors
/// Returns [`GatewayError::InvalidArguments`], [`GatewayError::Backend`], or
/// [`GatewayError::Collision`] for an invalid catalog lookup state.
pub fn route_tool(
    profile: &Profile,
    servers: &BTreeMap<String, Arc<dyn GatewayBackend>>,
    catalog: &Catalog,
    name: &str,
    arguments: Option<&RawValue>,
    identity_injection: bool,
) -> Result<CallToolResult, GatewayError> {
    static NEVER_CANCEL: AtomicBool = AtomicBool::new(false);
    let context = DispatchContext::new(&NEVER_CANCEL);
    route_tool_with_context(
        profile,
        servers,
        catalog,
        name,
        arguments,
        identity_injection,
        context,
    )
}

/// Forwards one exposed namespaced tool with the connection context.
///
/// # Errors
/// Returns the same errors as [`route_tool`].
pub fn route_tool_with_context(
    profile: &Profile,
    servers: &BTreeMap<String, Arc<dyn GatewayBackend>>,
    catalog: &Catalog,
    name: &str,
    arguments: Option<&RawValue>,
    identity_injection: bool,
    context: DispatchContext<'_>,
) -> Result<CallToolResult, GatewayError> {
    let entry = catalog
        .lookup(name)
        .filter(|entry| entry.verdict == symbrain_policy::Verdict::Exposed)
        .ok_or_else(|| GatewayError::UnknownTool(name.to_string()))?;
    forward_entry_with_context(
        profile,
        servers,
        entry,
        arguments,
        identity_injection,
        context,
    )
}

/// Forwards a catalog entry without performing another namespace lookup.
///
/// # Errors
/// Returns an unavailable-server or backend failure.
pub fn forward_entry(
    profile: &Profile,
    servers: &BTreeMap<String, Arc<dyn GatewayBackend>>,
    entry: &Entry,
    arguments: Option<&RawValue>,
    identity_injection: bool,
) -> Result<CallToolResult, GatewayError> {
    static NEVER_CANCEL: AtomicBool = AtomicBool::new(false);
    let context = DispatchContext::new(&NEVER_CANCEL);
    forward_entry_with_context(
        profile,
        servers,
        entry,
        arguments,
        identity_injection,
        context,
    )
}

/// Forwards a catalog entry with the connection context.
///
/// # Errors
/// Returns the same errors as [`forward_entry`].
pub fn forward_entry_with_context(
    profile: &Profile,
    servers: &BTreeMap<String, Arc<dyn GatewayBackend>>,
    entry: &Entry,
    arguments: Option<&RawValue>,
    identity_injection: bool,
    context: DispatchContext<'_>,
) -> Result<CallToolResult, GatewayError> {
    let backend = servers
        .get(&entry.server)
        .ok_or_else(|| GatewayError::UnknownServer(entry.server.clone()))?;
    let arguments = inject_identity(profile, &entry.server, arguments, identity_injection)?;
    backend
        .call_tool_with_context(&entry.original_name, arguments.as_deref(), context)
        .map_err(|source| GatewayError::Backend {
            server: entry.server.clone(),
            source,
        })
}

/// Injects a profile identity into an object argument, preserving caller wins.
/// Unmapped servers and disabled injection return an exact copy of the input.
///
/// # Errors
/// Returns an error when arguments are present but are not a JSON object.
pub fn inject_identity(
    profile: &Profile,
    alias: &str,
    input: Option<&RawValue>,
    enabled: bool,
) -> Result<Option<Box<RawValue>>, GatewayError> {
    if !enabled {
        return Ok(clone_raw(input));
    }
    let Some(parameter) = identity_parameter(alias) else {
        return Ok(clone_raw(input));
    };

    let mut args = match input {
        None => Map::new(),
        Some(raw) => match serde_json::from_str::<Value>(raw.get())
            .map_err(|error| GatewayError::InvalidArguments(error.to_string()))?
        {
            Value::Object(args) => args,
            _ => {
                return Err(GatewayError::InvalidArguments(
                    "expected a JSON object".to_string(),
                ));
            }
        },
    };
    if args.contains_key(parameter) {
        return Ok(clone_raw(input));
    }
    args.insert(parameter.to_string(), Value::String(profile.name.clone()));
    let encoded = serde_json::to_string(&Value::Object(args))
        .map_err(|error| GatewayError::InvalidArguments(error.to_string()))?;
    RawValue::from_string(encoded)
        .map(Some)
        .map_err(|error| GatewayError::InvalidArguments(error.to_string()))
}

fn clone_raw(input: Option<&RawValue>) -> Option<Box<RawValue>> {
    input.and_then(|raw| RawValue::from_string(raw.get().to_string()).ok())
}

/// Joins text content blocks using the gateway's newline separator.
#[must_use]
pub fn joined_text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}
