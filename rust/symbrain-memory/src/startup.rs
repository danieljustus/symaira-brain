//! Private Memory startup ownership, independent of gateway exposure policy.
use std::{io::Write, path::Path, sync::Arc};

use symbrain_core::GoText;

use crate::{Store, startup_fs, startup_secret};

/// Inputs resolved by the CLI after opening the Memory database. Shared secret
/// reference execution remains at the existing credential boundary.
pub struct SecretOptions {
    pub primary: Vec<u8>,
    pub path: Result<std::path::PathBuf, GoText>,
}

/// A database or JWT initialization failure; callers may degrade Memory only.
#[derive(Debug)]
pub enum StartupError {
    Database(GoText),
    Jwt(GoText),
}

/// Owns the private database and signing-key state for a connection. Creating
/// this owner does not expose tools or start an HTTP listener.
pub struct MemoryRuntime {
    store: Arc<Store>,
    _jwt: startup_secret::KeyState,
}

impl MemoryRuntime {
    /// Opens private Memory storage, then resolves/initializes the JWT owner.
    ///
    /// # Errors
    /// Returns the failing phase with its byte-valued Go diagnostic.
    pub fn open(
        database: &Path,
        secrets: impl FnOnce() -> Result<SecretOptions, GoText>,
        warnings: &mut dyn Write,
    ) -> Result<Self, StartupError> {
        let store = startup_fs::open_database(database).map_err(StartupError::Database)?;
        let options = secrets().map_err(StartupError::Jwt)?;
        let jwt = startup_secret::initialize(options, warnings).map_err(StartupError::Jwt)?;
        Ok(Self {
            store: Arc::new(store),
            _jwt: jwt,
        })
    }

    /// Returns the already-open store; gateway policy still determines tools.
    #[must_use]
    pub fn store(&self) -> Arc<Store> {
        Arc::clone(&self.store)
    }
}
