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
        .map_err(|e| UsageError::transport(&provider.id, &e))?;
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
