//! CLI selectors and the existing shared secret boundary for Memory startup.
use std::{io::Write, path::PathBuf, sync::Arc};

use symbrain_core::{GoText, go_path};
use symbrain_memory::{MemoryRuntime, SecretOptions, StartupError};

fn location(variable: &str, fallback: &str) -> Result<PathBuf, GoText> {
    let base = if let Some(root) =
        std::env::var_os(variable).filter(|root| PathBuf::from(root).is_absolute())
    {
        PathBuf::from(root)
    } else {
        let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        let home = std::env::var_os(name)
            .filter(|root| !root.is_empty())
            .ok_or_else(|| GoText::from(format!("{name} is not defined")))?;
        go_path::join(&[&home, std::ffi::OsStr::new(fallback)])
    };
    let current = go_path::join(&[base.as_os_str(), std::ffi::OsStr::new("symbrain/memory")]);
    let legacy = go_path::join(&[base.as_os_str(), std::ffi::OsStr::new("symmemory")]);
    Ok(if current.is_dir() || !legacy.is_dir() {
        current
    } else {
        legacy
    })
}

fn secret_options() -> Result<SecretOptions, GoText> {
    let config = super::config::load();
    let literal = go_path::os_bytes(&config.jwt_secret);
    let mut primary = symbrain_usage::resolve_reference_bytes(&literal, b"JWT_SECRET_KEY")
        .map_err(|error| error.with_prefix("JWT secret symvault:// resolution failed: "))?;
    if primary.is_empty() {
        primary = std::env::var_os("JWT_SECRET_KEY")
            .map(|value| go_path::os_bytes(&value))
            .unwrap_or_default();
    }
    let path = if config.jwt_secret_path.is_empty() {
        location("XDG_CONFIG_HOME", ".config").map(|directory| {
            go_path::join(&[directory.as_os_str(), std::ffi::OsStr::new("jwt.secret")])
        })
    } else {
        Ok(PathBuf::from(&config.jwt_secret_path))
    };
    Ok(SecretOptions { primary, path })
}

pub(crate) fn open_runtime(stderr: &mut dyn Write) -> Option<Arc<MemoryRuntime>> {
    let config = super::config::load();
    let path = if config.database.is_empty() {
        location("XDG_DATA_HOME", ".local/share")
            .map(|directory| {
                go_path::join(&[directory.as_os_str(), std::ffi::OsStr::new("default.db")])
            })
            .map_err(|error| error.with_prefix("failed to resolve database path: "))
    } else {
        Ok(PathBuf::from(&config.database))
    };
    let result = path
        .map_err(StartupError::Database)
        .and_then(|path| MemoryRuntime::open(&path, secret_options, stderr));
    match result {
        Ok(runtime) => Some(Arc::new(runtime)),
        Err(error) => {
            let (prefix, text) = match error {
                StartupError::Database(text) => {
                    (b"symbrain mcp: open memory db: ".as_slice(), text)
                }
                StartupError::Jwt(text) => {
                    (b"symbrain mcp: init memory JWT provider: ".as_slice(), text)
                }
            };
            let mut bytes = prefix.to_vec();
            bytes.extend(text.as_ref());
            bytes.push(b'\n');
            let _ = stderr.write(&bytes);
            None
        }
    }
}
