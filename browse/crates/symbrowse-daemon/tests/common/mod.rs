//! Shared integration-test helpers.
#![allow(dead_code)]

use std::sync::Arc;

use symbrowse_core::key_resolver::{KeySources, ProbeError};

/// In-memory state-key providers: a fixed test key via the environment source.
/// Neither `symvault` nor the Keychain is ever probed, so results do not
/// depend on the operator's credential stores.
pub struct IsolatedKeySources;

impl KeySources for IsolatedKeySources {
    fn vault(&self, _: &str) -> Result<Option<Vec<u8>>, ProbeError> {
        Ok(None)
    }
    fn keychain(&self, _: &str, _: &str) -> Result<Option<Vec<u8>>, ProbeError> {
        Ok(None)
    }
    fn environment(&self, name: &str) -> Option<String> {
        (name == symbrowse_core::key_resolver::ENV_KEY_NAME).then(|| "ab".repeat(32))
    }
}

#[must_use]
pub fn isolated_key_sources() -> Arc<dyn KeySources> {
    Arc::new(IsolatedKeySources)
}

/// Marks the re-executed child of [`isolated_environment_root`].
const ISOLATED_CHILD_ROOT: &str = "SYMBROWSE_TEST_ISOLATED_ROOT";

/// Runs the calling single-test binary inside an isolated process environment
/// without mutating the live process environment (`std::env::set_var` is
/// `unsafe` in edition 2024 and this workspace owns no unsafe code).
///
/// The first invocation (the parent) creates a fresh root under the system
/// temporary directory, re-executes the current test binary for exactly
/// `test_name` with `HOME`/`XDG_*` pointed into that root plus the hermetic
/// symbrowse policy variables, forwards its output, asserts that it succeeded
/// and returns `None`; the caller must then return immediately. The child
/// receives `Some(root)` and runs the real test body.
pub fn isolated_environment_root(prefix: &str, test_name: &str) -> Option<std::path::PathBuf> {
    if let Some(root) = std::env::var_os(ISOLATED_CHILD_ROOT) {
        return Some(std::path::PathBuf::from(root));
    }
    let root = std::env::temp_dir().join(format!("{prefix}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create isolated root");
    let status = std::process::Command::new(std::env::current_exe().expect("test binary path"))
        .args([test_name, "--exact", "--nocapture", "--test-threads=1"])
        .env(ISOLATED_CHILD_ROOT, &root)
        .env("HOME", root.join("home"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("XDG_STATE_HOME", root.join("xdg-state"))
        // Hermetic policy: no external guard binary, no desktop notification.
        .env("SYMBROWSE_SYMGUARD", "off")
        .env("SYMBROWSE_OOB_NOTIFY", "off")
        .env_remove("SYMBROWSE_MCP")
        .status()
        .expect("re-execute isolated test process");
    let _ = std::fs::remove_dir_all(&root);
    assert!(status.success(), "isolated test process failed: {status}");
    None
}
