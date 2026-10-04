//! Whole resolved admission shares Go's filesystem owner at every Vault boundary.
use std::path::{Path, PathBuf};
use symbrain_core::config::resolved::{self, ProcessSources, Sources};

pub(crate) fn valid_configuration() -> bool {
    resolved::load().is_ok()
}

pub(super) fn configured_override(path: &Path, variable: &str) -> Option<PathBuf> {
    let config = resolved::load_with_global_path(path, &OverrideSources(variable)).ok()?;
    (!config.servers.vault.is_empty()).then(|| {
        PathBuf::from(symbrain_core::go_path::from_bytes(
            config.servers.vault.as_ref(),
        ))
    })
}

// Tests may inject the Vault variable name. All owners and other source
// operations must remain the same as the real per-invocation Brain loader.
struct OverrideSources<'a>(&'a str);
impl Sources for OverrideSources<'_> {
    fn environment(&self, name: &str) -> Option<std::ffi::OsString> {
        ProcessSources.environment(if name == "SYMBRAIN_SERVERS_VAULT_BINARY_PATH" {
            self.0
        } else {
            name
        })
    }
    fn current_directory(&self) -> std::io::Result<PathBuf> {
        ProcessSources.current_directory()
    }
    fn metadata(&self, path: &Path) -> std::io::Result<()> {
        ProcessSources.metadata(path)
    }
    fn read(&self, path: &Path) -> Result<Vec<u8>, (&'static str, std::io::Error)> {
        ProcessSources.read(path)
    }
}
