//! Resolve the existing key providers before IPC; own one store per runtime.
use super::*;
use symbrowse_core::{
    key_resolver::{KeyResolver, KeySources, ResolveError},
    key_sources::SystemKeySources,
};

pub(super) fn initialize(spec: &SessionSpec) -> Result<Store, DaemonError> {
    initialize_with_sources(spec, SystemKeySources::default())
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
    #[cfg(windows)]
    {
        // Go os.MkdirAll uses syscall.ENOTDIR = ERROR_PATH_NOT_FOUND (3).
        // Use the real Windows message, stripping Rust's display-only suffix.
        let message = std::io::Error::from_raw_os_error(3).to_string();
        message
            .strip_suffix(" (os error 3)")
            .unwrap_or(&message)
            .to_owned()
    }
    #[cfg(not(windows))]
    {
        "not a directory".to_owned()
    }
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
