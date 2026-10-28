use super::{Provider, Request, UsageError};
use crate::{Cancellation, Transport, UsageSnapshot};
use std::sync::Arc;
#[cfg(test)]
#[path = "devin_quota_tests.rs"]
mod tests;

pub(super) fn request(provider: &Provider) -> Request {
    if provider.id == "devin" {
        let body = serde_json::json!({"metadata":{"apiKey":provider.credential(),"ideName":"devin","ideVersion":"1.108.2","extensionName":"devin","extensionVersion":"1.108.2","locale":"en"}}).to_string();
        return Request {
            provider_id: "devin".into(), method: "POST".into(),
            url: "https://server.codeium.com/exa.seat_management_pb.SeatManagementService/GetUserStatus".into(),
            headers: [("Content-Type".into(), "application/json".into()), ("Connect-Protocol-Version".into(), "1".into()), ("Accept".into(), "application/json".into())].into(),
            body: Some(body.into_bytes()),
        };
    }
    Request {
        provider_id: provider.id.clone(),
        method: "GET".into(),
        url: "https://api.z.ai/api/monitor/usage/quota/limit".into(),
        headers: [
            (
                "Authorization".into(),
                format!("Bearer {}", provider.credential()),
            ),
            ("Accept".into(), "application/json".into()),
        ]
        .into(),
        body: None,
    }
}
pub(super) fn fetch(
    provider: &Provider,
    transport: &Arc<dyn Transport>,
    cancel: &Cancellation,
) -> Result<UsageSnapshot, UsageError> {
    // Transport errors may echo the request (Devin carries its key in the body).
    let response = transport
        .request_with_cancel(request(provider), cancel)
        .map_err(|_| UsageError::transport(&provider.id, "quota request failed"))?;
    if !(200..300).contains(&response.status) {
        return Err(super::status_error(
            &provider.id,
            response.status,
            &response.headers,
        ));
    }
    crate::parser::parse_snapshot(&provider.id, "api", &response.body, chrono::Utc::now())
}
