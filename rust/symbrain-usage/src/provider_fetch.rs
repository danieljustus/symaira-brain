use super::{Provider, UsageError, status_error};
use crate::UsageSnapshot;
use crate::parser::parse_snapshot;
use crate::transport::{Cancellation, Request, Transport};
use chrono::Utc;
use std::sync::Arc;

pub(super) fn fetch_one(
    provider: &Provider,
    transport: &Arc<dyn Transport>,
    cancel: &Cancellation,
    source: &str,
    request: Request,
) -> Result<UsageSnapshot, UsageError> {
    let response = transport
        .request_with_cancel(request, cancel)
        .map_err(|e| {
            if provider.id == "grok" {
                UsageError::exact("Grok billing request failed; check your connection")
            } else {
                UsageError::transport(&provider.id, &e)
            }
        })?;
    if provider.id == "grok" {
        match response.status {
            401 | 403 => {
                return Err(UsageError::exact(
                    "Grok auth expired. Run `grok login` again; usage never refreshes or writes credentials.",
                ));
            }
            412 => {
                let team = serde_json::from_slice::<serde_json::Value>(&response.body)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("error")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_lowercase)
                    })
                    .is_some_and(|error| error.contains("no personal team"));
                if team {
                    return Err(UsageError::exact(
                        "Grok team billing unavailable: this account has no personal weekly quota",
                    ));
                }
            }
            _ => {}
        }
        if !(200..300).contains(&response.status) && response.status != 429 {
            return Err(UsageError::exact(format!(
                "Grok billing request failed with HTTP {}",
                response.status
            )));
        }
    }
    if !(200..300).contains(&response.status) {
        return Err(status_error(
            &provider.id,
            response.status,
            &response.headers,
        ));
    }
    let mut snapshot = parse_snapshot(&provider.id, source, &response.body, Utc::now())?;
    if provider.id == "moonshot" && provider.region == "cn" {
        snapshot.currency = Some("CNY".into());
        for meter in &mut snapshot.meters {
            meter.unit = "CNY".into();
        }
    }
    Ok(snapshot)
}
