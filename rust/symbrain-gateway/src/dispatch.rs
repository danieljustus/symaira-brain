use super::response::{ToolResultWire, tool_result};
use super::routing::{joined_text, route_tool_with_context};
use super::{Gateway, GatewayError, GatewayResponse};
use serde_json::Value;
use serde_json::value::RawValue;
use std::time::Instant;
use symbrain_mcp::{CODE_INVALID_PARAMS, DispatchContext};

impl Gateway {
    pub(super) fn handle_call(
        &self,
        id: Value,
        params: Option<&RawValue>,
        context: DispatchContext<'_>,
    ) -> Result<GatewayResponse, GatewayError> {
        let started = Instant::now();
        let params = match decode_params(params) {
            Ok(params) => params,
            Err(message) => return Ok(self.invalid_call(id, started, message)),
        };
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return Ok(self.invalid_call(id, started, "Invalid params: missing name".to_string()));
        };
        if let Some(response) = self.handle_builtin_call(id.clone(), name, &params, started)? {
            return Ok(response);
        }
        if name == "get_ai_usage" {
            return self.handle_usage_call(id, &params, context, started);
        }
        let arguments = params
            .get("arguments")
            .and_then(|value| RawValue::from_string(value.to_string()).ok());
        if self.memory_tool_names.iter().any(|tool| tool == name)
            || self.activity_tool_names.iter().any(|tool| tool == name)
        {
            return self.handle_embedded_call(id, name, &params, context, started);
        }
        self.handle_routed_call(id, name, arguments.as_deref(), context, started)
    }

    fn invalid_call(&self, id: Value, started: Instant, message: String) -> GatewayResponse {
        self.audit_error(
            "",
            None,
            started,
            "error",
            &GatewayError::InvalidArguments(message.clone()),
        );
        GatewayResponse::error(id, CODE_INVALID_PARAMS, message)
    }

    fn handle_builtin_call(
        &self,
        id: Value,
        name: &str,
        params: &serde_json::Map<String, Value>,
        started: Instant,
    ) -> Result<Option<GatewayResponse>, GatewayError> {
        let response = match name {
            "bootstrap" => Self::builtin_response(id, self.bootstrap()),
            "patterns" => Self::builtin_response(id, self.patterns()),
            _ => return Ok(None),
        };
        self.audit_builtin(name, params.get("arguments"), started, &response);
        Ok(Some(response?))
    }

    fn handle_usage_call(
        &self,
        id: Value,
        params: &serde_json::Map<String, Value>,
        context: DispatchContext<'_>,
        started: Instant,
    ) -> Result<GatewayResponse, GatewayError> {
        if !self.usage_allowed || self.usage.is_none() {
            let error = GatewayError::UnknownTool("get_ai_usage".to_string());
            let hidden_args = params
                .get("arguments")
                .and_then(|value| RawValue::from_string(value.to_string()).ok());
            self.audit_error(
                "get_ai_usage",
                hidden_args.as_deref(),
                started,
                "hidden",
                &error,
            );
            return Ok(GatewayResponse::error(
                id,
                symbrain_mcp::CODE_METHOD_NOT_FOUND,
                "Unknown tool: get_ai_usage",
            ));
        }
        let service = self.usage.as_ref().expect("usage checked above");
        let report = service.report_with_cancel(|| context.is_cancelled());
        let result = symbrain_usage::report_json(&report)
            .map_err(|error| GatewayError::Serialization(error.to_string()));
        let status = super::audit::usage_status(&report, result.is_ok());
        let classification = super::audit::usage_classification(&report, status);
        self.audit_usage(
            params.get("arguments"),
            started,
            status,
            classification.as_ref(),
        );
        match result {
            Ok(text) => GatewayResponse::success(id, tool_result(text, false)),
            Err(error) => GatewayResponse::success(id, tool_result(error.to_string(), true)),
        }
    }

    fn handle_embedded_call(
        &self,
        id: Value,
        name: &str,
        params: &serde_json::Map<String, Value>,
        context: DispatchContext<'_>,
        started: Instant,
    ) -> Result<GatewayResponse, GatewayError> {
        let response =
            match self.handle_embedded(id.clone(), name, params.get("arguments"), context) {
                Ok(response) => response,
                Err(error) => GatewayResponse::success(id, tool_result(error.to_string(), true))?,
            };
        let failed = response.error.is_some()
            || response
                .result
                .as_ref()
                .and_then(|raw| serde_json::from_str::<Value>(raw.get()).ok())
                .and_then(|value| value.get("isError").and_then(Value::as_bool))
                .unwrap_or(false);
        let (server, tool, exposure) = self.audit_target(name);
        let classification = symbrain_audit::Classification {
            category: "tool".to_string(),
            retryable: false,
        };
        let audit_args = params
            .get("arguments")
            .map_or_else(Vec::new, |value| value.to_string().into_bytes());
        self.audit_write(
            &server,
            &tool,
            &audit_args,
            started,
            if failed { "error" } else { "ok" },
            &exposure,
            failed.then_some(&classification),
        );
        Ok(response)
    }

    fn handle_routed_call(
        &self,
        id: Value,
        name: &str,
        arguments: Option<&RawValue>,
        context: DispatchContext<'_>,
        started: Instant,
    ) -> Result<GatewayResponse, GatewayError> {
        match route_tool_with_context(
            &self.profile,
            &self.servers,
            &self.catalog,
            name,
            arguments,
            self.identity_injection,
            context,
        ) {
            Ok(result) if result.is_error => {
                let text = format!("tool error: {}", joined_text(&result));
                self.audit_result(name, arguments, started, &result);
                GatewayResponse::success(id, tool_result(text, true))
            }
            Ok(result) => {
                self.audit_result(name, arguments, started, &result);
                GatewayResponse::success(id, ToolResultWire::from_result(&result))
            }
            Err(error @ GatewayError::UnknownTool(_)) => {
                let hidden = self
                    .catalog
                    .lookup(name)
                    .is_some_and(|entry| entry.verdict != symbrain_policy::Verdict::Exposed);
                self.audit_error(
                    name,
                    arguments,
                    started,
                    if hidden { "hidden" } else { "error" },
                    &error,
                );
                Ok(GatewayResponse::error(
                    id,
                    symbrain_mcp::CODE_METHOD_NOT_FOUND,
                    format!("Unknown tool: {name}"),
                ))
            }
            Err(error) => {
                let status = if error.classification().category == "cancelled" {
                    "cancelled"
                } else {
                    "error"
                };
                self.audit_error(name, arguments, started, status, &error);
                GatewayResponse::success(id, tool_result(error.to_string(), true))
            }
        }
    }
}

fn decode_params(params: Option<&RawValue>) -> Result<serde_json::Map<String, Value>, String> {
    let Some(params) = params else {
        return Err("Invalid params: missing params".to_string());
    };
    let value =
        serde_json::from_str::<Value>(params.get()).map_err(|_| "Invalid params".to_string())?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| "Invalid params".to_string())
}
