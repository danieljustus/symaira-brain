//! Schema-versioned, read-only AI provider usage reporting.
#![deny(unsafe_code)]

mod model;
mod parser;
mod providers;
mod transport;

pub use model::{AuthStatus, ProviderUsage, Report, UsageMeter, UsageSnapshot};
pub use providers::{
    Provider, ProviderSpec, UsageError, all_providers, is_secret_reference, is_vault_uri,
    needs_go_fallback, resolve_reference, resolve_reference_or_env, secretref_timeout,
    set_secretref_timeout,
};
pub use transport::{Cancellation, FixtureTransport, Request, Response, Transport, UreqTransport};

use std::collections::BTreeSet;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

pub const REPORT_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_PROVIDER_TIMEOUT: Duration = Duration::from_secs(8);
pub const MAX_CONCURRENT_PROVIDERS: usize = 4;
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(25);

pub struct Service {
    providers: Vec<Provider>,
    transport: Arc<dyn Transport>,
    provider_timeout: Duration,
    max_concurrency: usize,
}

impl Service {
    #[must_use]
    pub fn new() -> Self {
        Self {
            providers: all_providers(),
            transport: Arc::new(UreqTransport::default()),
            provider_timeout: DEFAULT_PROVIDER_TIMEOUT,
            max_concurrency: MAX_CONCURRENT_PROVIDERS,
        }
    }
    #[must_use]
    pub fn with_transport(providers: Vec<Provider>, transport: Arc<dyn Transport>) -> Self {
        Self {
            providers,
            transport,
            provider_timeout: DEFAULT_PROVIDER_TIMEOUT,
            max_concurrency: MAX_CONCURRENT_PROVIDERS,
        }
    }
    #[must_use]
    pub const fn with_provider_timeout(mut self, timeout: Duration) -> Self {
        self.provider_timeout = timeout;
        self
    }
    #[must_use]
    pub const fn with_max_concurrency(mut self, max: usize) -> Self {
        self.max_concurrency = if max == 0 { 1 } else { max };
        self
    }
    #[must_use]
    pub fn providers(&self) -> &[Provider] {
        &self.providers
    }
    #[must_use]
    pub fn report(&self) -> Report {
        self.report_with_cancel(|| false)
    }

    /// Runs bounded worker batches. `thread::scope` guarantees every worker is
    /// joined before return; cancellation is propagated before the join so a
    /// cooperative transport never survives a report call.
    #[must_use]
    pub fn report_with_cancel(&self, cancelled: impl Fn() -> bool) -> Report {
        self.report_with_cancel_until(cancelled, None)
    }

    /// Runs the report under an optional absolute deadline shared by all
    /// provider batches. Existing per-provider timeouts remain independently
    /// enforced when they expire first.
    #[must_use]
    pub fn report_with_cancel_until(
        &self,
        cancelled: impl Fn() -> bool,
        report_deadline: Option<Instant>,
    ) -> Report {
        let mut report = Report::new();
        report.providers = self
            .providers
            .iter()
            .map(ProviderUsage::from_provider)
            .collect();
        let configured: Vec<usize> = self
            .providers
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.configured.then_some(i))
            .collect();
        for (batch_index, batch) in configured.chunks(self.max_concurrency).enumerate() {
            // Recheck before starting each batch so cancellation or the
            // handler deadline never launches another provider request.
            if cancelled() || report_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                mark_unstarted_cancelled(
                    &mut report,
                    &self.providers,
                    &configured,
                    batch_index * self.max_concurrency,
                );
                break;
            }
            let provider_deadline = Instant::now() + self.provider_timeout;
            let batch_deadline = report_deadline.map_or(provider_deadline, |deadline| {
                deadline.min(provider_deadline)
            });
            let report_deadline_wins =
                report_deadline.is_some_and(|deadline| deadline <= provider_deadline);
            let (sender, receiver) = mpsc::channel();
            let cancels: Vec<Cancellation> = batch
                .iter()
                .map(|_| Cancellation::with_deadline(batch_deadline))
                .collect();
            let transport = Arc::clone(&self.transport);
            std::thread::scope(|scope| {
                let mut handles = Vec::new();
                for (slot, index) in batch.iter().copied().enumerate() {
                    let provider = self.providers[index].clone();
                    let transport = Arc::clone(&transport);
                    let cancel = cancels[slot].clone();
                    let sender = sender.clone();
                    handles.push(scope.spawn(move || {
                        let result = provider.fetch(&transport, &cancel);
                        let _ = sender.send((index, result));
                    }));
                }
                drop(sender);
                let mut pending: BTreeSet<usize> = batch.iter().copied().collect();
                while !pending.is_empty() {
                    if cancelled()
                        || report_deadline.is_some_and(|deadline| Instant::now() >= deadline)
                    {
                        for (slot, index) in batch.iter().copied().enumerate() {
                            if pending.contains(&index) {
                                cancels[slot].cancel();
                                report.providers[index].error = Some(format!(
                                    "AI usage provider {:?} cancelled",
                                    self.providers[index].id
                                ));
                            }
                        }
                        break;
                    }
                    let remaining = batch_deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        for (slot, index) in batch.iter().copied().enumerate() {
                            if pending.contains(&index) {
                                cancels[slot].cancel();
                                report.providers[index].error = Some(timeout_error(
                                    &self.providers[index],
                                    report_deadline_wins,
                                    self.provider_timeout,
                                ));
                            }
                        }
                        break;
                    }
                    match receiver.recv_timeout(remaining.min(CANCELLATION_POLL_INTERVAL)) {
                        Ok((index, result)) => {
                            pending.remove(&index);
                            match result {
                                Ok(snapshot) => report.providers[index].snapshot = Some(snapshot),
                                Err(error) => {
                                    report.providers[index].error = Some(error.to_string());
                                }
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                for handle in handles {
                    let _ = handle.join();
                }
            });
        }
        report
    }
}

fn mark_unstarted_cancelled(
    report: &mut Report,
    providers: &[Provider],
    configured: &[usize],
    first_unstarted: usize,
) {
    for index in configured.iter().skip(first_unstarted).copied() {
        report.providers[index].error = Some(format!(
            "AI usage provider {:?} cancelled",
            providers[index].id
        ));
    }
}

fn timeout_error(
    provider: &Provider,
    report_deadline_wins: bool,
    provider_timeout: Duration,
) -> String {
    if report_deadline_wins {
        format!("AI usage provider {:?} cancelled", provider.id)
    } else {
        format!(
            "AI usage provider {:?} timed out after {:?}",
            provider.id, provider_timeout
        )
    }
}

impl Default for Service {
    fn default() -> Self {
        Self::new()
    }
}
#[must_use]
pub fn build_report() -> Report {
    Service::new().report()
}
/// Serializes a usage report using the stable schema-v1 wire format.
///
/// # Errors
/// Returns the JSON serialization error when the report cannot be encoded.
pub fn report_json(report: &Report) -> Result<String, serde_json::Error> {
    serde_json::to_string(report)
}

#[cfg(test)]
mod deadline_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    #[derive(Default)]
    struct WaitingTransport {
        requests: AtomicUsize,
    }

    impl Transport for WaitingTransport {
        fn request(&self, _request: Request) -> Result<Response, String> {
            Err("expected cancellation-aware request".to_string())
        }

        fn request_with_cancel(
            &self,
            _request: Request,
            cancel: &Cancellation,
        ) -> Result<Response, String> {
            self.requests.fetch_add(1, Ordering::AcqRel);
            while !cancel.is_cancelled() {
                thread::sleep(Duration::from_millis(2));
            }
            Err("request cancelled".to_string())
        }
    }

    #[test]
    fn absolute_report_deadline_cancels_inflight_and_skips_later_batch() {
        let transport = Arc::new(WaitingTransport::default());
        let service = Service::with_transport(
            vec![
                Provider::fixture("claude", "Claude"),
                Provider::fixture("codex", "Codex"),
            ],
            transport.clone(),
        )
        .with_provider_timeout(Duration::from_secs(60))
        .with_max_concurrency(1);
        let deadline = Instant::now() + Duration::from_millis(100);
        let started = Instant::now();
        let report = service.report_with_cancel_until(|| false, Some(deadline));

        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(transport.requests.load(Ordering::Acquire), 1);
        assert!(
            report.providers[0]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("cancelled"))
        );
        assert!(
            report.providers[1]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("cancelled"))
        );
    }

    #[test]
    fn provider_timeout_keeps_timeout_error_classification() {
        let transport = Arc::new(WaitingTransport::default());
        let service =
            Service::with_transport(vec![Provider::fixture("claude", "Claude")], transport)
                .with_provider_timeout(Duration::from_millis(100));
        let report = service.report_with_cancel_until(|| false, None);

        assert!(
            report.providers[0]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("timed out after 100ms"))
        );
    }
}
