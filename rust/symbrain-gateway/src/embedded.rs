//! Native embedded memory and activity tool dispatch.

mod activity;
mod catalog;
mod common;
mod memory;
mod memory_catalog;
mod skills;
mod skills_catalog;
mod skills_config;
mod skills_tools;

use crate::{Gateway, GatewayError, GatewayResponse};
use serde_json::{Value, json};
use symbrain_mcp::DispatchContext;

pub(crate) use catalog::{exposed_native, exposed_skills};
pub(crate) use skills_tools::handles as is_profile_or_versioning_tool;

impl Gateway {
    /// Profile, discovery and per-skill versioning tools: they decode the raw
    /// `arguments` member themselves (Go's typed `json.Unmarshal`).
    pub(crate) fn handle_skills_versioning(
        id: Value,
        name: &str,
        raw: Option<&serde_json::value::RawValue>,
        context: DispatchContext<'_>,
    ) -> Result<GatewayResponse, GatewayError> {
        common::check_cancel(context)?;
        let response = skills_tools::response(id, name, raw)?;
        common::check_cancel(context)?;
        Ok(response)
    }

    pub(crate) fn handle_embedded(
        &self,
        id: Value,
        name: &str,
        args: Option<&Value>,
        context: DispatchContext<'_>,
    ) -> Result<GatewayResponse, GatewayError> {
        common::check_cancel(context)?;
        let value = args.cloned().unwrap_or_else(|| json!({}));
        let result = if catalog::MEMORY_TOOLS.contains(&name) {
            let store = self
                .memory
                .as_ref()
                .ok_or_else(|| GatewayError::UnknownTool(name.to_string()))?;
            common::dispatch_memory(store, name, &value)
        } else if catalog::ACTIVITY_TOOLS.contains(&name) {
            let store = self
                .memory
                .as_ref()
                .ok_or_else(|| GatewayError::UnknownTool(name.to_string()))?;
            common::dispatch_activity(store, name, &value)
        } else if catalog::SKILLS_TOOLS.contains(&name) {
            skills::dispatch(name, &value)
        } else {
            Err(GatewayError::UnknownTool(name.to_string()))
        };
        common::check_cancel(context)?;
        match result {
            Ok(text) => common::text_response(id, text, false),
            Err(error) => common::text_response(id, error.to_string(), true),
        }
    }
}
