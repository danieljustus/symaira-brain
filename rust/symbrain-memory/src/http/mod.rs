//! Shared, bounded native memory HTTP owner.
//!
//! The native `memory serve` runtime: every route runs the same native
//! pipelines as the CLI against the one embedded store. There is no hidden Go
//! process and no second store.

use crate::{EmbeddingGenerator, Store, StoreError};
use std::sync::Arc;

mod auth;
mod authority;
mod go_json;
mod log;
mod middleware;
mod read;
mod rfc3339;
mod routes;
mod server;
mod stats;
mod sync;
mod sync_decode;
#[cfg(test)]
mod tests;
mod wire;
mod write;

/// Options explicitly owned by the caller's memory runtime snapshot.
#[allow(clippy::struct_excessive_bools)] // Independent switches of the shipped config.
pub struct Options {
    /// Version returned by the public status endpoint.
    pub version: String,
    /// Deny write access when the JWT subject has no stored profile.
    pub require_profile: bool,
    /// Whether SQLite mutation auditing is enabled.
    pub audit_enabled: bool,
    /// `security.trusted_proxies`: CIDRs whose forwarded client address is
    /// used as the rate-limit key.
    pub trusted_proxies: Vec<String>,
    /// `hybrid_search.quantize_to_binary` for synced vectors.
    pub quantize_binary: bool,
    /// `hybrid_search.prefilter_enabled`.
    pub prefilter: bool,
    /// The write-path conflict configuration.
    pub conflict: crate::ConflictConfig,
    /// `working_memory.ttl` after the shipped default.
    pub working_ttl: std::time::Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            version: "dev".into(),
            require_profile: false,
            audit_enabled: true,
            trusted_proxies: Vec::new(),
            quantize_binary: false,
            prefilter: false,
            conflict: crate::ConflictConfig {
                enabled: false,
                ..crate::ConflictConfig::default()
            },
            working_ttl: std::time::Duration::from_hours(24),
        }
    }
}

/// Native HTTP lifecycle bound to the same embedded Store as other clients.
/// Secrets are never formatted by this owner or sent to its public status route.
pub struct Server {
    store: Arc<Store>,
    auth: auth::Auth,
    generator: EmbeddingGenerator,
    options: Options,
    limiter: middleware::Limiter,
    listener_port: std::sync::atomic::AtomicU16,
    metrics: stats::Metrics,
}

impl Server {
    /// Creates a native owner with explicit secret material from its caller.
    ///
    /// This constructor does not resolve credentials, open another database,
    /// bind a port, spawn workers or enable writes implicitly.
    ///
    /// # Errors
    /// Rejects an empty authentication secret.
    pub fn new(
        store: Arc<Store>,
        secret: Vec<u8>,
        generator: EmbeddingGenerator,
        options: Options,
    ) -> Result<Self, StoreError> {
        if secret.is_empty() {
            return Err(StoreError::Invalid("memory HTTP secret is empty".into()));
        }
        Ok(Self {
            store,
            auth: auth::Auth::new(secret),
            generator,
            limiter: middleware::Limiter::new(
                options
                    .trusted_proxies
                    .iter()
                    .filter_map(|cidr| middleware::Cidr::parse(cidr))
                    .collect(),
            ),
            options,
            listener_port: std::sync::atomic::AtomicU16::new(0),
            metrics: stats::Metrics::default(),
        })
    }
}
