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
use provider_requests::{command_output_with_cancel, request_for};
#[path = "provider_fetch.rs"]
mod provider_fetch;
#[path = "provider_opencode.rs"]
mod provider_opencode;
use provider_fetch::fetch_one;

#[path = "provider_config.rs"]
mod provider_config;
pub use provider_config::{
    all_providers, is_secret_reference, is_vault_uri, needs_go_fallback, resolve_reference,
    resolve_reference_or_env, secretref_timeout, set_secretref_timeout,
};
#[cfg(test)]
pub(crate) use provider_config::{
    claude_from_resolved, codex_from_resolved, copilot_from_resolved, cursor_from_resolved,
    kimi_from_resolved, moonshot_from_resolved, nous_from_resolved, opencode_from_resolved,
    openrouter_from_resolved,
};

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
    /// Device id the Kimi Code CLI persisted; sent as identity metadata on the
    /// CLI strategy's requests.
    device_id: Option<String>,
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
            device_id: None,
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
        let result = if self.fixture && self.id == "antigravity" && self.credential.is_none() {
            Err(UsageError::not_running())
        } else if self.fixture && self.id == "antigravity" {
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
        wrap_fetch_error(&self.id, result)
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
        fetch_antigravity_from_observations(self, transport, cancel, &process_list, |pid| {
            let pid = pid.to_string();
            command_output_with_cancel(
                cancel,
                "lsof",
                &["-nP", "-iTCP", "-sTCP:LISTEN", "-a", "-p", &pid],
            )
        })
    }
}

fn wrap_fetch_error(
    provider_id: &str,
    result: Result<UsageSnapshot, UsageError>,
) -> Result<UsageSnapshot, UsageError> {
    result.map_err(|error| match error {
        UsageError::Chain { .. } => error,
        error => UsageError::chain(provider_id, vec![error]),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AntigravityCandidate {
    pid: u32,
    csrf_token: Option<String>,
}

fn parse_antigravity_candidates(process_list: &str) -> Vec<AntigravityCandidate> {
    let mut candidates = Vec::new();
    for line in process_list.split('\n') {
        let line = line.trim_start_matches(' ');
        let Some((pid, command)) = line.split_once(' ') else {
            continue;
        };
        let Ok(pid) = pid.parse::<u32>() else {
            continue;
        };
        let command = command.trim_start_matches(' ');
        let lower = command.to_ascii_lowercase();
        let server = (lower.contains("language_server") || lower.contains("language-server"))
            && (lower.contains("antigravity")
                || (lower.contains("--app_data_dir") && command.contains("antigravity")));
        let cli = lower.contains("/agy")
            || lower.contains("antigravity-cli")
            || lower.contains("antigravity_cli");
        if !server && !cli {
            continue;
        }
        let csrf_token = command.find("--csrf_token").and_then(|index| {
            command[index + "--csrf_token".len()..]
                .trim_start_matches(' ')
                .split(' ')
                .next()
                .filter(|token| !token.is_empty())
                .map(str::to_owned)
        });
        candidates.push(AntigravityCandidate { pid, csrf_token });
    }
    candidates
}

fn parse_antigravity_ports(port_list: &str) -> Vec<u32> {
    let mut ports = Vec::new();
    for line in port_list
        .split('\n')
        .filter(|line| line.contains("(LISTEN)"))
    {
        let bytes = line.as_bytes();
        for (index, byte) in bytes.iter().enumerate() {
            if *byte != b':' {
                continue;
            }
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() && end - start < 5 {
                end += 1;
            }
            if end == start || (end < bytes.len() && !bytes[end].is_ascii_whitespace()) {
                continue;
            }
            let Ok(port_text) = std::str::from_utf8(&bytes[start..end]) else {
                continue;
            };
            let Ok(port) = port_text.parse::<u32>() else {
                continue;
            };
            if !ports.contains(&port) {
                ports.push(port);
            }
            break;
        }
    }
    ports
}

fn fetch_antigravity_from_observations(
    provider: &Provider,
    transport: &Arc<dyn Transport>,
    cancel: &Cancellation,
    process_list: &str,
    mut ports_for_pid: impl FnMut(u32) -> Option<String>,
) -> Result<UsageSnapshot, UsageError> {
    let candidates = parse_antigravity_candidates(process_list);
    if candidates.is_empty() {
        return Err(UsageError::not_running());
    }
    let mut last_error = UsageError::not_running();
    for candidate in candidates {
        // Go refuses to pass PIDs above its deliberately bounded probe limit
        // to lsof; keep the same ceiling before invoking the local tool.
        if candidate.pid > i32::MAX as u32 {
            continue;
        }
        let Some(port_output) = ports_for_pid(candidate.pid) else {
            continue;
        };
        for port in parse_antigravity_ports(&port_output) {
            match fetch_antigravity_from_port(
                provider,
                transport,
                cancel,
                port,
                candidate.csrf_token.as_deref(),
            ) {
                Ok(snapshot) => return Ok(snapshot),
                Err(error) => last_error = error,
            }
        }
    }
    Err(last_error)
}

fn fetch_antigravity_from_port(
    provider: &Provider,
    transport: &Arc<dyn Transport>,
    cancel: &Cancellation,
    port: u32,
    csrf_token: Option<&str>,
) -> Result<UsageSnapshot, UsageError> {
    let connect = antigravity_request(provider, port, "GetUnleashData", csrf_token);
    let response = transport
        .request_with_cancel(connect, cancel)
        .map_err(|_| {
            UsageError::exact(format!(
                "Antigravity probe failed: connect probe failed on port {port}"
            ))
        })?;
    if !(200..300).contains(&response.status) {
        return Err(UsageError::exact(format!(
            "Antigravity probe failed: connect probe failed on port {port}"
        )));
    }

    let mut last_error = UsageError::parse(&provider.id, "all quota endpoints failed");
    for endpoint in [
        "RetrieveUserQuotaSummary",
        "GetUserStatus",
        "GetCommandModelConfigs",
    ] {
        let request = antigravity_request(provider, port, endpoint, csrf_token);
        match transport.request_with_cancel(request, cancel) {
            Ok(response) if (200..300).contains(&response.status) => {
                match parse_snapshot(&provider.id, "local", &response.body, Utc::now()) {
                    Ok(snapshot) => return Ok(snapshot),
                    Err(_) => {
                        last_error = UsageError::exact(format!(
                            "Antigravity returned an unreadable response: {endpoint} returned unreadable data"
                        ));
                    }
                }
            }
            Ok(response) => {
                last_error = status_error(&provider.id, response.status, &response.headers);
            }
            Err(error) => last_error = UsageError::transport(&provider.id, &error),
        }
    }
    Err(last_error)
}

fn antigravity_request(
    provider: &Provider,
    port: u32,
    endpoint: &str,
    csrf_token: Option<&str>,
) -> Request {
    let mut request = request_for(
        provider,
        "local",
        "",
        Some(&format!("https://127.0.0.1:{port}/{endpoint}")),
    );
    if endpoint == "GetUnleashData" {
        request.body = None;
        request.headers.remove("Accept");
        request.headers.remove("Content-Type");
        request
            .headers
            .insert("Connect-Protocol-Version".into(), "1".into());
    }
    if let Some(token) = csrf_token {
        request
            .headers
            .insert("X-Codeium-Csrf-Token".into(), token.into());
    }
    request
}

#[cfg(test)]
#[path = "opencode_discovery_tests.rs"]
mod opencode_discovery_tests;

#[cfg(test)]
#[path = "antigravity_tests.rs"]
mod antigravity_tests;
