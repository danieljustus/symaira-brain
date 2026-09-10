//! Native gateway domain: catalog assembly, MCP dispatch, and child routing.

#![deny(unsafe_code)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use serde_json::value::RawValue;
use serde_json::{Value, json};
use symbrain_broker::{CallToolResult, ManagedServer};
use symbrain_mcp::{CODE_METHOD_NOT_FOUND, DispatchContext, Request};
use symbrain_policy::Profile;
use symbrain_usage::Service;

mod audit;
mod builtin;
mod catalog;
mod dispatch;
mod dispatcher_impl;
mod embedded;
mod error;
mod response;
mod routing;

pub use catalog::{CatalogAssembly, build_catalog};
pub use error::{BackendError, GatewayError, join_content};
pub use response::{GatewayResponse, ListedTool, ToolAnnotations};
use response::{ToolsResult, builtin_tool, usage_tool};
pub use routing::{
    forward_entry, forward_entry_with_context, inject_identity, joined_text, route_tool,
    route_tool_with_context,
};

/// Backend abstraction used by the domain and by deterministic tests.
pub trait GatewayBackend: Send + Sync {
    /// Returns the backend's live MCP tool definitions.
    ///
    /// # Errors
    /// Returns the backend failure when listing tools fails.
    fn list_tools(&self) -> Result<Vec<symbrain_broker::Tool>, BackendError>;
    /// Calls the original, unnamespaced child tool.
    ///
    /// # Errors
    /// Returns the backend failure when the child call fails.
    fn call_tool(
        &self,
        name: &str,
        arguments: Option<&RawValue>,
    ) -> Result<CallToolResult, BackendError>;

    /// Context-aware child call seam. Backends that can cancel their own work
    /// may override this; the default preserves existing backend adapters.
    ///
    /// # Errors
    /// Returns backend failures from the child call.
    fn call_tool_with_context(
        &self,
        name: &str,
        arguments: Option<&RawValue>,
        _context: DispatchContext<'_>,
    ) -> Result<CallToolResult, BackendError> {
        self.call_tool(name, arguments)
    }
}

impl GatewayBackend for ManagedServer {
    fn list_tools(&self) -> Result<Vec<symbrain_broker::Tool>, BackendError> {
        ManagedServer::list_tools(self).map_err(BackendError::from)
    }

    fn call_tool(
        &self,
        name: &str,
        arguments: Option<&RawValue>,
    ) -> Result<CallToolResult, BackendError> {
        ManagedServer::call_tool(self, name, arguments).map_err(BackendError::from)
    }

    fn call_tool_with_context(
        &self,
        name: &str,
        arguments: Option<&RawValue>,
        context: DispatchContext<'_>,
    ) -> Result<CallToolResult, BackendError> {
        ManagedServer::call_tool_with_cancel(self, name, arguments, &|| context.is_cancelled())
            .map_err(BackendError::from)
    }
}

/// A fully assembled gateway connection. Child catalogs are immutable after
/// construction, so routing cannot expose a tool absent from `tools/list`.
pub struct Gateway {
    profile: Profile,
    servers: BTreeMap<String, Arc<dyn GatewayBackend>>,
    catalog: symbrain_catalog::Catalog,
    degradations: Vec<symbrain_audit::Degradation>,
    version: String,
    identity_injection: bool,
    audit: Option<Arc<symbrain_audit::Logger>>,
    usage: Option<Arc<Service>>,
    usage_allowed: bool,
    memory: Option<Arc<symbrain_memory::Store>>,
    memory_tool_names: Vec<String>,
    activity_tool_names: Vec<String>,
    audit_failure_reported: AtomicBool,
}

impl Gateway {
    /// Assembles a gateway from a validated profile and live child backends.
    ///
    /// # Errors
    /// Returns policy failures or namespaced tool collisions.
    pub fn new(
        profile: Profile,
        servers: BTreeMap<String, Arc<dyn GatewayBackend>>,
        version: impl Into<String>,
    ) -> Result<Self, GatewayError> {
        let assembly = build_catalog(&profile, &servers)?;
        let usage_config = profile.server(symbrain_policy::SERVER_USAGE);
        let usage_report = symbrain_policy::evaluate(
            symbrain_policy::SERVER_USAGE,
            &usage_config,
            &["get_ai_usage".to_string()],
        )
        .map_err(|error| GatewayError::Policy(error.to_string()))?;
        let usage_allowed = usage_report
            .exposed
            .iter()
            .any(|name| name == "get_ai_usage");
        let usage = usage_allowed.then(|| Arc::new(Service::new()));
        // A supplied memory backend is the legacy/fixture transport. Prefer it
        // over the embedded store so a connection never publishes or routes two
        // implementations of the same unnamespaced core tools. Production
        // `mcp` intentionally does not create a memory child, so it takes the
        // embedded path below.
        let has_memory_backend = servers.contains_key(symbrain_policy::SERVER_MEMORY);
        let (memory_tool_names, activity_tool_names) = if has_memory_backend {
            (Vec::new(), Vec::new())
        } else {
            embedded::exposed_native(&profile)
        };
        let memory = if !has_memory_backend
            && profile.server(symbrain_policy::SERVER_MEMORY).enabled
            && (!memory_tool_names.is_empty() || !activity_tool_names.is_empty())
        {
            let path = std::env::var_os("SYMBRAIN_MEMORY_DB")
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    symbrain_core::xdg::data_dir().map(|dir| dir.join("memory").join("default.db"))
                })
                .ok_or_else(|| {
                    GatewayError::Policy("resolve embedded memory database path".into())
                })?;
            Some(Arc::new(symbrain_memory::Store::open(&path).map_err(
                |error| GatewayError::Policy(format!("open embedded memory store: {error}")),
            )?))
        } else {
            None
        };
        Ok(Self {
            profile,
            servers,
            catalog: assembly.catalog,
            degradations: assembly.degradations,
            version: version.into(),
            identity_injection: true,
            audit: None,
            usage,
            usage_allowed,
            memory,
            memory_tool_names,
            activity_tool_names,
            audit_failure_reported: AtomicBool::new(false),
        })
    }

    /// Builds a gateway from managed broker servers.
    ///
    /// # Errors
    /// Returns policy failures or namespaced tool collisions.
    pub fn from_managed(
        profile: Profile,
        servers: BTreeMap<String, Arc<ManagedServer>>,
        version: impl Into<String>,
    ) -> Result<Self, GatewayError> {
        let servers = servers
            .into_iter()
            .map(|(name, server)| {
                let backend: Arc<dyn GatewayBackend> = server;
                (name, backend)
            })
            .collect();
        Self::new(profile, servers, version)
    }

    /// Disables or enables memory `client_id` injection.
    #[must_use]
    pub const fn with_identity_injection(mut self, enabled: bool) -> Self {
        self.identity_injection = enabled;
        self
    }

    /// Attaches the profile audit sink. Audit failures are best-effort and
    /// never change an MCP response.
    #[must_use]
    pub fn with_audit(mut self, audit: Option<Arc<symbrain_audit::Logger>>) -> Self {
        if let Some(logger) = &audit {
            for degradation in &self.degradations {
                logger.log_degradation(
                    &degradation.server,
                    &degradation.reason,
                    &degradation.level,
                );
            }
        }
        self.audit = audit;
        self
    }

    /// Replaces the production usage service with an injected deterministic
    /// service while retaining the profile policy decision.
    #[must_use]
    pub fn with_usage_service(mut self, service: Arc<Service>) -> Self {
        if self.usage_allowed {
            self.usage = Some(service);
        }
        self
    }

    /// Returns the immutable child catalog.
    #[must_use]
    pub const fn catalog(&self) -> &symbrain_catalog::Catalog {
        &self.catalog
    }

    /// Returns non-fatal startup degradations in deterministic server order.
    #[must_use]
    pub fn degradations(&self) -> &[symbrain_audit::Degradation] {
        &self.degradations
    }

    /// Returns the complete `tools/list` shape, including gateway-owned tools.
    #[must_use]
    pub fn tools(&self) -> Vec<ListedTool> {
        let mut tools = vec![builtin_tool("bootstrap"), builtin_tool("patterns")];
        if self.usage_allowed && self.usage.is_some() {
            tools.push(usage_tool());
        }
        tools.extend(self.catalog.exposed().map(ListedTool::from_entry));
        tools.extend(self.embedded_tools());
        tools
    }

    /// Dispatches one already-decoded JSON-RPC request. This is the domain
    /// seam; stdio framing and server-loop lifecycle belong to P5-2.
    ///
    /// # Errors
    /// Returns only assembly-independent domain failures. Request-level
    /// errors are encoded as JSON-RPC responses.
    pub fn handle(&self, request: &Request) -> Result<Option<GatewayResponse>, GatewayError> {
        static NEVER_CANCEL: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        let context = DispatchContext::new(&NEVER_CANCEL);
        self.handle_with_context(request, context)
    }

    /// Dispatches one request with the connection cancellation context.
    ///
    /// # Errors
    /// Returns only assembly-independent domain failures. Request-level
    /// errors are encoded as JSON-RPC responses.
    pub fn handle_with_context(
        &self,
        request: &Request,
        context: DispatchContext<'_>,
    ) -> Result<Option<GatewayResponse>, GatewayError> {
        if request.is_notification() {
            return Ok(None);
        }
        let id = request.id.clone().unwrap_or(Value::Null);
        match request.method.as_str() {
            "initialize" => GatewayResponse::success(id, self.initialize_result()).map(Some),
            "ping" => GatewayResponse::success(id, json!({})).map(Some),
            "tools/list" => GatewayResponse::success(
                id,
                ToolsResult {
                    tools: self.tools(),
                },
            )
            .map(Some),
            "tools/call" => self
                .handle_call(id, request.params.as_deref(), context)
                .map(Some),
            _ => Ok(Some(GatewayResponse::error(
                id,
                CODE_METHOD_NOT_FOUND,
                format!("Method not found: {}", request.method),
            ))),
        }
    }

    fn initialize_result(&self) -> Value {
        json!({
            "protocolVersion": symbrain_mcp::PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "symbrain", "version": self.version },
            "instructions": self.instructions(),
        })
    }
}
