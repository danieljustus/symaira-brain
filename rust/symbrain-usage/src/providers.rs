use crate::parser::parse_snapshot;
use crate::transport::{Cancellation, Request, Transport};
use crate::{AuthStatus, UsageSnapshot};
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;

#[path = "provider_errors.rs"]
mod provider_errors;
pub use provider_errors::UsageError;
use provider_errors::status_error;
#[path = "provider_requests.rs"]
mod provider_requests;
pub(crate) use provider_requests::trusted_https_url;
use provider_requests::{
    command_output_with_cancel, extract_workspace, normalize_workspace, request_for,
};
#[path = "provider_fetch.rs"]
mod provider_fetch;
use provider_fetch::fetch_one;

#[path = "provider_config.rs"]
mod provider_config;
pub use provider_config::all_providers;

const MAX_CREDENTIAL_FILE_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSpec {
    pub id: String,
    pub display_name: String,
    pub credential_sources: Vec<&'static str>,
}

#[derive(Debug, Clone)]
pub struct Provider {
    pub spec: ProviderSpec,
    pub id: String,
    pub display_name: String,
    pub configured: bool,
    pub auth_status: AuthStatus,
    credential: Option<String>,
    credentials: Vec<(String, String)>,
    base_url: Option<String>,
    region: String,
    fixture: bool,
}

impl Provider {
    fn new(
        id: &str,
        display_name: &str,
        sources: Vec<&'static str>,
        credentials: Vec<(String, String)>,
        status: AuthStatus,
    ) -> Self {
        let credential = credentials.first().map(|(_, v)| v.clone());
        Self {
            spec: ProviderSpec {
                id: id.into(),
                display_name: display_name.into(),
                credential_sources: sources,
            },
            id: id.into(),
            display_name: display_name.into(),
            configured: credential.is_some() || id == "antigravity",
            auth_status: status,
            credential,
            credentials,
            base_url: None,
            region: "ai".into(),
            fixture: false,
        }
    }

    /// A configured provider that performs no process, home, keychain, or network reads.
    #[must_use]
    pub fn fixture(id: &str, display_name: &str) -> Self {
        let mut p = Self::new(
            id,
            display_name,
            Vec::new(),
            vec![("fixture".into(), "fixture".into())],
            AuthStatus {
                status: "available".into(),
                detail: "configured by fixture".into(),
                source: Some("fixture".into()),
            },
        );
        p.fixture = true;
        if id == "opencode" {
            p.credentials
                .push(("workspace".into(), "wrk_fixture".into()));
        }
        p
    }

    pub(crate) fn fetch(
        &self,
        transport: &Arc<dyn Transport>,
        cancel: &Cancellation,
    ) -> Result<UsageSnapshot, UsageError> {
        let result = if self.fixture && self.id == "antigravity" {
            self.fetch_antigravity_fixture(transport, cancel)
        } else if self.fixture {
            let source = match self.id.as_str() {
                "claude" | "codex" => "oauth",
                "opencode" | "cursor" => "web",
                "antigravity" => "local",
                _ => "api",
            };
            fetch_one(
                self,
                transport,
                cancel,
                source,
                request_for(
                    self,
                    source,
                    if self.id == "opencode" {
                        ""
                    } else {
                        self.credential()
                    },
                    (self.id == "opencode").then_some("wrk_fixture"),
                ),
            )
        } else {
            match self.id.as_str() {
                "claude" => self.fetch_claude(transport, cancel),
                "codex" => self.fetch_one(
                    transport,
                    cancel,
                    "oauth",
                    request_for(self, "oauth", self.credential(), None),
                ),
                "copilot" | "moonshot" | "nous" | "openrouter" => self.fetch_one(
                    transport,
                    cancel,
                    "api",
                    request_for(self, "api", self.credential(), None),
                ),
                "cursor" => self.fetch_one(
                    transport,
                    cancel,
                    "web",
                    request_for(self, "web", self.credential(), None),
                ),
                "kimi" => self.fetch_kimi(transport, cancel),
                "opencode" => self.fetch_opencode(transport, cancel),
                "antigravity" => self.fetch_antigravity(transport, cancel),
                _ => Err(UsageError::parse(&self.id, "unknown provider")),
            }
        };
        result.map_err(|error| match error {
            UsageError::Chain { .. } => error,
            error => UsageError::chain(&self.id, vec![error]),
        })
    }

    fn credential(&self) -> &str {
        self.credential.as_deref().unwrap_or("")
    }
    fn fetch_one(
        &self,
        transport: &Arc<dyn Transport>,
        cancel: &Cancellation,
        source: &str,
        request: Request,
    ) -> Result<UsageSnapshot, UsageError> {
        fetch_one(self, transport, cancel, source, request)
    }
    fn fetch_claude(
        &self,
        transport: &Arc<dyn Transport>,
        cancel: &Cancellation,
    ) -> Result<UsageSnapshot, UsageError> {
        let mut errors = Vec::new();
        for (source, credential) in &self.credentials {
            let req = request_for(self, source, credential, None);
            match fetch_one(self, transport, cancel, source, req) {
                Ok(v) => return Ok(v),
                Err(e) => errors.push(e),
            }
        }
        Err(UsageError::chain(&self.id, errors))
    }
    fn fetch_kimi(
        &self,
        transport: &Arc<dyn Transport>,
        cancel: &Cancellation,
    ) -> Result<UsageSnapshot, UsageError> {
        let mut errors = Vec::new();
        for (source, credential) in &self.credentials {
            let req = request_for(self, source, credential, None);
            match fetch_one(self, transport, cancel, source, req) {
                Ok(v) => return Ok(v),
                Err(e) => errors.push(e),
            }
        }
        Err(UsageError::chain(&self.id, errors))
    }
    fn fetch_opencode(
        &self,
        transport: &Arc<dyn Transport>,
        cancel: &Cancellation,
    ) -> Result<UsageSnapshot, UsageError> {
        let cookie = self.credential();
        let workspace = self
            .credentials
            .iter()
            .find(|(s, _)| s == "workspace")
            .map_or("", |(_, value)| value.as_str());
        let workspace = normalize_workspace(workspace);
        let id = if workspace.is_empty() {
            let response = transport
                .request_with_cancel(request_for(self, "workspace_get", cookie, None), cancel)
                .map_err(|e| UsageError::transport(&self.id, &e))?;
            if !(200..300).contains(&response.status) {
                return Err(status_error(&self.id, response.status, &response.headers));
            }
            extract_workspace(&response.body)
                .ok_or_else(|| UsageError::parse(&self.id, "missing workspace id"))?
        } else {
            workspace
        };
        let response = transport
            .request_with_cancel(request_for(self, "web", cookie, Some(&id)), cancel)
            .map_err(|e| UsageError::transport(&self.id, &e))?;
        if !(200..300).contains(&response.status) {
            return Err(status_error(&self.id, response.status, &response.headers));
        }
        parse_snapshot(&self.id, "web", &response.body, Utc::now()).or_else(|_| {
            let fallback = request_for(self, "web_post", cookie, Some(&id));
            let response = transport
                .request_with_cancel(fallback, cancel)
                .map_err(|e| UsageError::transport(&self.id, &e))?;
            if !(200..300).contains(&response.status) {
                return Err(status_error(&self.id, response.status, &response.headers));
            }
            parse_snapshot(&self.id, "web", &response.body, Utc::now())
        })
    }
    fn fetch_antigravity_fixture(
        &self,
        transport: &Arc<dyn Transport>,
        cancel: &Cancellation,
    ) -> Result<UsageSnapshot, UsageError> {
        let port = 43123_u16;
        let connect = request_for(
            self,
            "local",
            "",
            Some(&format!("https://127.0.0.1:{port}/GetUnleashData")),
        );
        let response = transport
            .request_with_cancel(connect, cancel)
            .map_err(|error| {
                UsageError::exact(format!(
                    "Antigravity probe failed: connect probe failed on port {port}: {error}"
                ))
            })?;
        if !(200..300).contains(&response.status) {
            return Err(UsageError::exact(format!(
                "Antigravity probe failed: connect probe failed on port {port}"
            )));
        }
        let mut last_error = UsageError::parse(&self.id, "all quota endpoints failed");
        for endpoint in [
            "RetrieveUserQuotaSummary",
            "GetUserStatus",
            "GetCommandModelConfigs",
        ] {
            let request = request_for(
                self,
                "local",
                "",
                Some(&format!("https://127.0.0.1:{port}/{endpoint}")),
            );
            match transport.request_with_cancel(request, cancel) {
                Ok(response) if (200..300).contains(&response.status) => {
                    match parse_snapshot(&self.id, "local", &response.body, Utc::now()) {
                        Ok(snapshot) => return Ok(snapshot),
                        Err(_) => {
                            last_error = UsageError::exact(format!(
                                "Antigravity returned an unreadable response: {endpoint} returned unreadable data"
                            ));
                        }
                    }
                }
                Ok(_) => {
                    last_error = UsageError::exact(format!(
                        "Antigravity probe failed: connect probe failed on port {port}"
                    ));
                }
                Err(error) => {
                    last_error = UsageError::exact(format!(
                        "Antigravity probe failed: connect probe failed on port {port}: {error}"
                    ));
                }
            }
        }
        Err(last_error)
    }

    fn fetch_antigravity(
        &self,
        transport: &Arc<dyn Transport>,
        cancel: &Cancellation,
    ) -> Result<UsageSnapshot, UsageError> {
        let process_list =
            command_output_with_cancel(cancel, "ps", &["-ax", "-o", "pid=,command="])
                .unwrap_or_default();
        let csrf_token = process_list.lines().find_map(|line| {
            let marker = "--csrf_token";
            let start = line.find(marker)? + marker.len();
            line[start..].split_whitespace().next().map(str::to_owned)
        });
        let pid = process_list.lines().find_map(|line| {
            let lower = line.to_ascii_lowercase();
            if !lower.contains("agy") && !lower.contains("antigravity") {
                return None;
            }
            line.split_whitespace().next()?.parse::<u32>().ok()
        });
        let Some(pid) = pid else {
            return Err(UsageError::not_running());
        };
        let pid_text = pid.to_string();
        let ports = command_output_with_cancel(
            cancel,
            "lsof",
            &["-nP", "-iTCP", "-sTCP:LISTEN", "-a", "-p", &pid_text],
        )
        .unwrap_or_default();
        let port = ports.lines().find_map(|line| {
            line.contains("(LISTEN)")
                .then(|| {
                    line.rsplit_once(':')?
                        .1
                        .split_whitespace()
                        .next()?
                        .parse::<u16>()
                        .ok()
                })
                .flatten()
        });
        let Some(port) = port else {
            return Err(UsageError::not_running());
        };
        let mut last_error = UsageError::parse(&self.id, "all quota endpoints failed");
        for endpoint in [
            "RetrieveUserQuotaSummary",
            "GetUserStatus",
            "GetCommandModelConfigs",
        ] {
            let mut req = request_for(
                self,
                "local",
                "",
                Some(&format!("https://127.0.0.1:{port}/{endpoint}")),
            );
            if let Some(token) = csrf_token.as_deref() {
                req.headers
                    .insert("X-Codeium-CSRF-Token".into(), token.into());
            }
            match transport.request_with_cancel(req, cancel) {
                Ok(response) if (200..300).contains(&response.status) => {
                    match parse_snapshot(&self.id, "local", &response.body, Utc::now()) {
                        Ok(v) => return Ok(v),
                        Err(e) => last_error = e,
                    }
                }
                Ok(response) => {
                    last_error = status_error(&self.id, response.status, &response.headers);
                }
                Err(e) => last_error = UsageError::transport(&self.id, &e),
            }
        }
        Err(last_error)
    }
}
