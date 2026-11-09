//! The configuration path exactly as the Go loader prints it.
//!
//! corekit's `configkit.DefaultPath` returns `filepath.Join(...)`; on the
//! supported Unix targets the printed text is the path bytes unchanged.
use std::path::Path;

use symbrain_core::config::os_bytes;

/// Bytes Go prints for `path`.
pub(super) fn printed(path: &Path) -> Vec<u8> {
    let bytes = os_bytes(path.as_os_str());
    bytes.into_owned()
}

#[cfg(test)]
#[path = "setup_config_path_tests.rs"]
mod tests;
