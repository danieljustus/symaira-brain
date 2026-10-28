use super::{Provider, UsageError, request_for, status_error};
use crate::UsageSnapshot;
use crate::parser::{parse_snapshot, parser_openrouter};
use crate::transport::{Cancellation, Request, Response, Transport};
use chrono::Utc;
use std::sync::Arc;

impl Provider {
    pub(super) fn fetch_openrouter(
        &self,
        transport: &Arc<dyn Transport>,
        cancel: &Cancellation,
    ) -> Result<UsageSnapshot, UsageError> {
        let request = request_for(self, "credits", self.credential(), None);
        let response = send(transport, cancel, request)?;
        // Endpoint absence alone permits the legacy API, never bad auth or malformed credits.
        if response.status == 404 {
            let response = send(
                transport,
                cancel,
                request_for(self, "legacy", self.credential(), None),
            )?;
            check_status(&response)?;
            return parse_snapshot("openrouter", "api", &response.body, Utc::now());
        }
        check_status(&response)?;
        let mut snapshot = parse_snapshot("openrouter", "credits", &response.body, Utc::now())?;
        // Leave time to publish credits even if supplemental key lookup stalls.
        let budget = cancel
            .remaining()
            .map_or(std::time::Duration::from_secs(1), |remaining| {
                remaining
                    .saturating_sub(std::time::Duration::from_millis(100))
                    .min(std::time::Duration::from_secs(1))
            });
        if budget.is_zero() || cancel.is_cancelled() {
            return Ok(snapshot);
        }
        let optional = Cancellation::with_deadline(std::time::Instant::now() + budget);
        if let Ok(response) = send(
            transport,
            &optional,
            request_for(self, "key", self.credential(), None),
        ) && (200..300).contains(&response.status)
        {
            parser_openrouter::enrich(&response.body, &mut snapshot);
        }
        Ok(snapshot)
    }
}

fn send(
    transport: &Arc<dyn Transport>,
    cancel: &Cancellation,
    request: Request,
) -> Result<Response, UsageError> {
    transport
        .request_with_cancel(request, cancel)
        .map_err(|_| UsageError::transport("openrouter", "provider transport failed"))
}

fn check_status(response: &Response) -> Result<(), UsageError> {
    if !(200..300).contains(&response.status) {
        return Err(status_error(
            "openrouter",
            response.status,
            &response.headers,
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "openrouter_current_tests.rs"]
mod tests;
