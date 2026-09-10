use super::response::tool_result;
use super::{Gateway, GatewayError, GatewayResponse};
use serde_json::Value;
use serde_json::value::RawValue;
use std::sync::atomic::Ordering;
use std::time::Instant;
use symbrain_broker::CallToolResult;

pub(super) fn usage_status(report: &symbrain_usage::Report, serialized: bool) -> &'static str {
    if !serialized {
        return "error";
    }
    let has_error = report
        .providers
        .iter()
        .any(|provider| provider.error.is_some());
    if !has_error {
        return "ok";
    }
    if report.providers.iter().any(|provider| {
        provider
            .error
            .as_deref()
            .is_some_and(|error| error.contains("cancelled"))
    }) {
        return "cancelled";
    }
    if report
        .providers
        .iter()
        .any(|provider| provider.snapshot.is_some())
    {
        "partial"
    } else {
        "error"
    }
}

pub(super) fn usage_classification(
    report: &symbrain_usage::Report,
    status: &str,
) -> Option<symbrain_audit::Classification> {
    match status {
        "ok" => None,
        "cancelled" => Some(symbrain_audit::Classification {
            category: "cancelled".to_string(),
            retryable: false,
        }),
        _ if report
            .providers
            .iter()
            .any(|provider| provider.error.is_some()) =>
        {
            Some(symbrain_audit::Classification {
                category: "tool".to_string(),
                retryable: false,
            })
        }
        _ => Some(symbrain_audit::Classification {
            category: "internal".to_string(),
            retryable: false,
        }),
    }
}

impl Gateway {
    pub(super) fn audit_usage(
        &self,
        arguments: Option<&Value>,
        started: Instant,
        status: &str,
        classification: Option<&symbrain_audit::Classification>,
    ) {
        let args = arguments.map_or_else(Vec::<u8>::new, |value| value.to_string().into_bytes());
        self.audit_write(
            "usage",
            "get_ai_usage",
            &args,
            started,
            status,
            &symbrain_audit::Exposure::default(),
            classification,
        );
    }

    pub(super) fn audit_builtin(
        &self,
        name: &str,
        arguments: Option<&Value>,
        started: Instant,
        response: &Result<GatewayResponse, GatewayError>,
    ) {
        let args = arguments.map_or_else(Vec::<u8>::new, |value| value.to_string().into_bytes());
        let status = if response.is_ok() { "ok" } else { "error" };
        let classification = response.as_ref().err().map(GatewayError::classification);
        self.audit_write(
            "symbrain",
            name,
            &args,
            started,
            status,
            &symbrain_audit::Exposure::default(),
            classification.as_ref(),
        );
    }

    pub(super) fn audit_result(
        &self,
        name: &str,
        arguments: Option<&RawValue>,
        started: Instant,
        result: &CallToolResult,
    ) {
        let (server, tool, exposure) = self.audit_target(name);
        let classification = symbrain_audit::Classification {
            category: "tool".to_string(),
            retryable: false,
        };
        self.audit_write(
            &server,
            &tool,
            arguments.map_or(&[], |value| value.get().as_bytes()),
            started,
            if result.is_error { "error" } else { "ok" },
            &exposure,
            result.is_error.then_some(&classification),
        );
    }

    pub(super) fn audit_error(
        &self,
        name: &str,
        arguments: Option<&RawValue>,
        started: Instant,
        status: &str,
        error: &GatewayError,
    ) {
        let (server, tool, exposure) = self.audit_target(name);
        let classification = error.classification();
        self.audit_write(
            &server,
            &tool,
            arguments.map_or(&[], |value| value.get().as_bytes()),
            started,
            status,
            &exposure,
            Some(&classification),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn audit_write(
        &self,
        server: &str,
        tool: &str,
        args: &[u8],
        started: Instant,
        status: &str,
        exposure: &symbrain_audit::Exposure,
        classification: Option<&symbrain_audit::Classification>,
    ) {
        if let Some(logger) = &self.audit {
            logger.log(
                server,
                tool,
                args,
                started.elapsed(),
                status,
                exposure,
                classification,
            );
            if logger.degraded() && !self.audit_failure_reported.swap(true, Ordering::AcqRel) {
                eprintln!(
                    "symbrain mcp: audit logging degraded; subsequent entries may be dropped"
                );
            }
        }
    }

    pub(super) fn audit_target(&self, name: &str) -> (String, String, symbrain_audit::Exposure) {
        if let Some(entry) = self.catalog.lookup(name) {
            return (
                entry.server.clone(),
                entry.original_name.clone(),
                symbrain_audit::Exposure {
                    access_class: entry.access_class.clone(),
                    access_source: entry.access_source.clone(),
                },
            );
        }
        if name == "get_ai_usage" {
            return (
                "usage".to_string(),
                "get_ai_usage".to_string(),
                symbrain_audit::Exposure::default(),
            );
        }
        if let Some(tool) = name.strip_prefix("vault_") {
            return (
                "vault".to_string(),
                tool.to_string(),
                symbrain_audit::Exposure::default(),
            );
        }
        (
            "unknown".to_string(),
            name.to_string(),
            symbrain_audit::Exposure::default(),
        )
    }

    pub(super) fn builtin_response(
        id: Value,
        value: Result<String, GatewayError>,
    ) -> Result<GatewayResponse, GatewayError> {
        match value {
            Ok(value) => GatewayResponse::success(id, tool_result(value, false)),
            Err(error) => GatewayResponse::success(id, tool_result(error.to_string(), true)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{usage_classification, usage_status};
    use symbrain_usage::{AuthStatus, ProviderUsage, Report, UsageSnapshot};

    fn provider(snapshot: bool, error: Option<&str>) -> ProviderUsage {
        ProviderUsage {
            id: "fixture".into(),
            display_name: "Fixture".into(),
            configured: true,
            auth_status: AuthStatus::default(),
            snapshot: snapshot.then(UsageSnapshot::default),
            error: error.map(str::to_owned),
        }
    }

    #[test]
    fn usage_audit_status_distinguishes_success_partial_and_cancelled() {
        assert_eq!(usage_status(&Report::default(), true), "ok");
        let partial = Report {
            schema_version: 1,
            providers: vec![provider(true, None), provider(false, Some("parse failed"))],
        };
        assert_eq!(usage_status(&partial, true), "partial");
        assert_eq!(
            usage_classification(&partial, "partial")
                .expect("partial classification")
                .category,
            "tool"
        );
        let cancelled = Report {
            schema_version: 1,
            providers: vec![provider(false, Some("provider cancelled"))],
        };
        assert_eq!(usage_status(&cancelled, true), "cancelled");
        assert_eq!(
            usage_classification(&cancelled, "cancelled")
                .expect("cancelled classification")
                .category,
            "cancelled"
        );
    }
}
