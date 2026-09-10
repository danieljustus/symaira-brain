//! Schema-versioned, read-only AI provider usage reporting.
#![deny(unsafe_code)]

mod model;
mod parser;
mod providers;
mod transport;

pub use model::{AuthStatus, ProviderUsage, Report, UsageMeter, UsageSnapshot};
pub use providers::{Provider, ProviderSpec, UsageError, all_providers};
pub use transport::{Cancellation, FixtureTransport, Request, Response, Transport, UreqTransport};

use std::collections::BTreeSet;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

pub const REPORT_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_PROVIDER_TIMEOUT: Duration = Duration::from_secs(8);
pub const MAX_CONCURRENT_PROVIDERS: usize = 4;

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

    /// Runs bounded worker batches. Workers are detached deliberately: a
    /// transport implementation may not honor cooperative cancellation, and
    /// joining such a worker would let it extend the report deadline. The
    /// worker owns only its cloned transport/cancellation state and can no
    /// longer mutate the returned report after the receiver is dropped.
    #[must_use]
    pub fn report_with_cancel(&self, cancelled: impl Fn() -> bool) -> Report {
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
        for batch in configured.chunks(self.max_concurrency) {
            let (sender, receiver) = mpsc::channel();
            let cancels: Vec<Cancellation> = batch
                .iter()
                .map(|_| Cancellation::with_timeout(self.provider_timeout))
                .collect();
            let transport = Arc::clone(&self.transport);
            for (slot, index) in batch.iter().copied().enumerate() {
                let provider = self.providers[index].clone();
                let transport = Arc::clone(&transport);
                let cancel = cancels[slot].clone();
                let sender = sender.clone();
                std::thread::spawn(move || {
                    let result = provider.fetch(&transport, &cancel);
                    let _ = sender.send((index, result));
                });
            }
            drop(sender);
            let mut pending: BTreeSet<usize> = batch.iter().copied().collect();
            let deadline = Instant::now() + self.provider_timeout;
            while !pending.is_empty() {
                if cancelled() {
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
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    for (slot, index) in batch.iter().copied().enumerate() {
                        if pending.contains(&index) {
                            cancels[slot].cancel();
                            report.providers[index].error = Some(format!(
                                "AI usage provider {:?} timed out after {:?}",
                                self.providers[index].id, self.provider_timeout
                            ));
                        }
                    }
                    break;
                }
                match receiver.recv_timeout(remaining) {
                    Ok((index, result)) => {
                        pending.remove(&index);
                        match result {
                            Ok(snapshot) => report.providers[index].snapshot = Some(snapshot),
                            Err(error) => {
                                report.providers[index].error = Some(error.to_string());
                            }
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        for (slot, index) in batch.iter().copied().enumerate() {
                            if pending.contains(&index) {
                                cancels[slot].cancel();
                                report.providers[index].error = Some(format!(
                                    "AI usage provider {:?} timed out after {:?}",
                                    self.providers[index].id, self.provider_timeout
                                ));
                            }
                        }
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        }
        report
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
