//! Resolve the existing key providers before IPC; own one store per runtime.
use super::*;
use symbrowse_core::{
    key_resolver::{KeyResolver, KeySources, ProbeError, ResolveError},
    key_sources::SystemKeySources,
};

/// Wraps injected providers so they satisfy the by-value `KeySources` bound.
struct SharedSources(std::sync::Arc<dyn KeySources>);

impl KeySources for SharedSources {
    fn vault(&self, entry: &str) -> Result<Option<Vec<u8>>, ProbeError> {
        self.0.vault(entry)
    }
    fn keychain(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, ProbeError> {
        self.0.keychain(service, account)
    }
    fn environment(&self, name: &str) -> Option<String> {
        self.0.environment(name)
    }
}

pub(super) fn initialize(
    spec: &SessionSpec,
    owner: Option<std::path::PathBuf>,
    injected: Option<std::sync::Arc<dyn KeySources>>,
) -> Result<Store, DaemonError> {
    if let Some(injected) = injected {
        return initialize_with_sources(spec, SharedSources(injected));
    }
    let sources = SystemKeySources::default();
    let sources = match owner {
        Some(owner) => sources.with_startup_owner(owner),
        None => sources,
    };
    initialize_with_sources(spec, sources)
}

pub(super) fn initialize_with_sources(
    spec: &SessionSpec,
    sources: impl KeySources,
) -> Result<Store, DaemonError> {
    let key = KeyResolver::new(sources).resolve().map_err(|error| {
        runtime_error(format!(
            "resolve state encryption key: {}",
            source_error(error)
        ))
    })?;
    Store::new(
        spec.state_store_dir(),
        time::Duration::days(spec.state_expire_days),
        key,
    )
    .map_err(|error| {
        let message = match error {
            symbrowse_core::state_store::StoreError::UnsafeFileType(path) => format!(
                "create state directory: mkdir {}: {}",
                path.display(),
                not_directory_message()
            ),
            error => format!("create state directory: {error}"),
        };
        runtime_error(message)
    })
}

fn not_directory_message() -> String {
    "not a directory".to_owned()
}

fn source_error(error: ResolveError) -> String {
    // Go surfaces provider deadline errors outside its vault-entry context.
    // The existing Core runner owns the actual timeout and child-tree cleanup.
    let prefix = "symvault entry \"symbrowse/encryption-key\": ";
    if error.0.strip_prefix(prefix).is_some_and(|message| {
        message.starts_with("command timed out")
            || message == "stdout pipe remained open after command exit"
    }) {
        "context deadline exceeded".to_owned()
    } else {
        error.0
    }
}

#[cfg(test)]
#[path = "state_key_tests.rs"]
pub(super) mod tests;
