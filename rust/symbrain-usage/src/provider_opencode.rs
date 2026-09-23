//! `OpenCode` workspace/subscription request state machine.
use super::provider_requests::{
    extract_workspace, looks_signed_out, normalize_workspace, request_for,
};
use super::{Provider, UsageError, status_error};
use crate::UsageSnapshot;
use crate::parser::parse_snapshot;
use crate::transport::{Cancellation, Response, Transport};
use chrono::Utc;
use std::sync::Arc;

impl Provider {
    pub(super) fn fetch_opencode(
        &self,
        transport: &Arc<dyn Transport>,
        cancel: &Cancellation,
    ) -> Result<UsageSnapshot, UsageError> {
        // Go's OpenCodeProvider.Strategies() returns nil without
        // OPENCODE_COOKIE — a workspace override alone marks the provider
        // configured but yields no strategy — so the shipped chain fails with
        // an empty failure list.
        let cookie = self
            .credentials
            .iter()
            .find(|(source, value)| source != "workspace" && !value.is_empty())
            .map_or("", |(_, value)| value.as_str());
        if cookie.is_empty() {
            return Err(UsageError::chain(&self.id, Vec::new()));
        }
        let workspace = self
            .credentials
            .iter()
            .find(|(source, _)| source == "workspace")
            .map_or("", |(_, value)| value.as_str());
        let workspace = normalize_workspace(workspace);
        let id = if workspace.is_empty() {
            self.discover_workspace(cookie, transport, cancel)?
        } else {
            workspace
        };
        let text = self.subscription_text(&id, cookie, transport, cancel)?;
        // openCodeParseSubscription reports every failed subscription parse as
        // `missing usage fields`, whichever layer could not read the body.
        parse_snapshot(&self.id, "web", text.as_bytes(), Utc::now())
            .map_err(|_| UsageError::parse(&self.id, "missing usage fields"))
    }

    /// Ports `fetchWorkspaceID`: GET the workspaces server, scan the body
    /// for an id, then retry once with the `[]` POST when none was found.
    /// Signed-out bodies fail in `opencode_response_text` first.
    fn discover_workspace(
        &self,
        cookie: &str,
        transport: &Arc<dyn Transport>,
        cancel: &Cancellation,
    ) -> Result<String, UsageError> {
        let response = transport
            .request_with_cancel(request_for(self, "workspace_get", cookie, None), cancel)
            .map_err(|error| opencode_network_error(&error))?;
        let text = opencode_response_text(&self.id, &response)?;
        if let Some(id) = extract_workspace(text.as_bytes()) {
            return Ok(id);
        }
        let response = transport
            .request_with_cancel(request_for(self, "workspace_post", cookie, None), cancel)
            .map_err(|error| opencode_network_error(&error))?;
        let text = opencode_response_text(&self.id, &response)?;
        extract_workspace(text.as_bytes())
            .ok_or_else(|| UsageError::parse(&self.id, "missing workspace id"))
    }

    /// Ports `fetchSubscriptionInfo`: GET the subscription and fall back to
    /// the POST when the GET body does not parse; signed-out bodies fail in
    /// `opencode_response_text` before the parse-check. The final parse of
    /// whichever body is returned happens in `fetch_opencode`.
    fn subscription_text(
        &self,
        id: &str,
        cookie: &str,
        transport: &Arc<dyn Transport>,
        cancel: &Cancellation,
    ) -> Result<String, UsageError> {
        let response = transport
            .request_with_cancel(request_for(self, "web", cookie, Some(id)), cancel)
            .map_err(|error| opencode_network_error(&error))?;
        let text = opencode_response_text(&self.id, &response)?;
        if parse_snapshot(&self.id, "web", text.as_bytes(), Utc::now()).is_ok() {
            return Ok(text);
        }
        let response = transport
            .request_with_cancel(request_for(self, "web_post", cookie, Some(id)), cancel)
            .map_err(|error| opencode_network_error(&error))?;
        opencode_response_text(&self.id, &response)
    }
}

/// Ports `serverText`: any non-2xx status maps through `status_error`, with
///401/403 and signed-out bodies (at any status) replaced by the shipped
/// invalid-credentials error. Only a clean 2xx body is returned as text.
fn opencode_response_text(id: &str, response: &Response) -> Result<String, UsageError> {
    let text = String::from_utf8_lossy(&response.body).into_owned();
    if !(200..300).contains(&response.status) {
        if matches!(response.status, 401 | 403) || looks_signed_out(&text) {
            return Err(opencode_invalid_credentials());
        }
        return Err(status_error(id, response.status, &response.headers));
    }
    if looks_signed_out(&text) {
        return Err(opencode_invalid_credentials());
    }
    Ok(text)
}

/// The exact invalid-credential message the shipped `openCodeError` emits for
/// `invalid_credentials` (stale `OPENCODE_COOKIE`).
fn opencode_invalid_credentials() -> UsageError {
    UsageError::exact(
        "OpenCode session cookie is invalid or expired. Re-import the opencode.ai cookie.",
    )
}

/// The exact network wrapper the shipped code puts around every transport
/// failure in the `OpenCode` flow (`openCodeError` kind=network).
fn opencode_network_error(detail: &str) -> UsageError {
    UsageError::exact(format!("OpenCode request failed: {detail}"))
}
