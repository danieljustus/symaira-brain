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
