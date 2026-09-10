//! Live child catalog assembly and startup degradation reporting.

use std::collections::BTreeMap;
use std::sync::Arc;

use symbrain_audit::Degradation;
use symbrain_catalog::{Catalog, ServerTools, Tool as CatalogTool};
use symbrain_policy::{ForeignTool, Profile, ToolAnnotations, evaluate, evaluate_foreign};

use crate::{GatewayBackend, GatewayError};

/// Result of querying enabled backends and merging their policy-shaped tools.
#[derive(Debug)]
pub struct CatalogAssembly {
    /// Immutable, sorted child catalog.
    pub catalog: Catalog,
    /// Non-fatal child failures and empty foreign exposures.
    pub degradations: Vec<Degradation>,
}

/// Builds the live gateway catalog. Disabled servers are not queried, while
/// list failures are recorded as degradations so one missing child cannot
/// prevent the gateway from starting.
///
/// # Errors
/// Returns a policy error or a namespaced tool collision.
pub fn build_catalog(
    profile: &Profile,
    servers: &BTreeMap<String, Arc<dyn GatewayBackend>>,
) -> Result<CatalogAssembly, GatewayError> {
    let mut server_tools = Vec::new();
    let mut degradations = Vec::new();

    for (alias, backend) in servers {
        let config = profile.server(alias);
        if !config.enabled {
            continue;
        }

        let tools = match backend.list_tools() {
            Ok(tools) => tools,
            Err(error) => {
                degradations.push(Degradation {
                    server: alias.clone(),
                    reason: error.to_string(),
                    level: "warning".to_string(),
                    ..Degradation::default()
                });
                continue;
            }
        };

        let live_names: Vec<String> = tools.iter().map(|tool| tool.name.clone()).collect();
        let report = if symbrain_policy::is_core_alias(alias) {
            evaluate(alias, &config, &live_names)
        } else {
            let foreign_tools = tools
                .iter()
                .map(|tool| ForeignTool {
                    name: tool.name.clone(),
                    read_only_hint: read_only_hint(tool),
                    annotations: annotations(tool),
                })
                .collect::<Vec<_>>();
            evaluate_foreign(alias, &config, &foreign_tools)
        }
        .map_err(|error| GatewayError::Policy(error.to_string()))?;

        if !symbrain_policy::is_core_alias(alias)
            && config.access == symbrain_policy::FOREIGN_ACCESS_READ
            && !tools.is_empty()
            && report.exposed.is_empty()
        {
            degradations.push(Degradation {
                server: alias.clone(),
                reason: format!(
                    "access=read exposes 0 of {} tools: none is classified as reading (no readOnlyHint from the upstream server and no tools_read override) — add tools_read entries for the tools that should be exposed",
                    tools.len()
                ),
                level: "warning".to_string(),
                ..Degradation::default()
            });
        }

        let translated = tools.iter().map(to_catalog_tool).collect();
        server_tools.push(ServerTools {
            server: alias.clone(),
            tools: translated,
            report,
        });
    }

    Ok(CatalogAssembly {
        catalog: Catalog::build(&server_tools)?,
        degradations,
    })
}

fn to_catalog_tool(tool: &symbrain_broker::Tool) -> CatalogTool {
    CatalogTool {
        name: tool.name.clone(),
        description: tool.description.clone(),
        input_schema: tool.input_schema.clone(),
        annotations: annotations(tool),
    }
}

fn annotations(tool: &symbrain_broker::Tool) -> Option<ToolAnnotations> {
    tool.annotations
        .as_ref()
        .and_then(|value| serde_json::from_value(value.clone()).ok())
}

fn read_only_hint(tool: &symbrain_broker::Tool) -> Option<bool> {
    annotations(tool).and_then(|value| value.read_only_hint)
}
