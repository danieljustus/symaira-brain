//! Shared, bounded native memory HTTP owner.
//!
//! This positive UI/API slice deliberately does not admit the complete legacy
//! `memory serve` CLI: sync, full configuration and unrestricted write contracts
//! retain their separately recorded migration gates. Unsupported operations
//! fail explicitly; there is no hidden Go process or second store.

use crate::{EmbeddingGenerator, Store, StoreError};
use std::sync::Arc;

mod auth;
mod middleware;
mod read;
mod routes;
mod server;
mod wire;
mod write;

/// Options explicitly owned by the caller's memory runtime snapshot.
pub struct Options {
    /// Version returned by the public status endpoint.
    pub version: String,
    /// Deny write access when the JWT subject has no stored profile.
    pub require_profile: bool,
    /// Enable only the proven extraction-free, conflict-disabled direct writes.
    pub direct_writes: bool,
    /// Whether SQLite mutation auditing is enabled.
    pub audit_enabled: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            version: "dev".into(),
            require_profile: false,
            direct_writes: false,
            audit_enabled: true,
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
            options,
            limiter: middleware::Limiter::default(),
        })
    }
}
