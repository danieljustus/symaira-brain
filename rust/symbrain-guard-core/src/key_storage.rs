//! Guard capability-key generation and persistence.
//!
//! This module mirrors the Go key-file boundary. It deliberately does not
//! derive or cache a signing key: callers receive the persisted master bytes
//! and choose when to pass them to [`crate::capability::derive_key`].

use std::{
    env, fmt, fs, io,
    path::{Path, PathBuf},
};

use crate::capability::KEY_SIZE;

/// Errors from the capability key-file boundary.
#[derive(Debug)]
pub enum KeyError {
    /// The key file or its parent could not be accessed.
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    /// Existing key material is shorter than the required master-key size.
    NoKeyMaterial { path: PathBuf, length: usize },
    /// The operating system could not provide cryptographically secure bytes.
    Random(String),
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                f,
                "capability: {operation} key {}: {source}",
                path.display()
            ),
            Self::NoKeyMaterial { path, length } => write!(
                f,
                "capability: no key material: key file {} is {length} bytes, want at least {KEY_SIZE}",
                path.display()
            ),
            Self::Random(source) => write!(f, "capability: generate key: {source}"),
        }
    }
}

impl std::error::Error for KeyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::NoKeyMaterial { .. } | Self::Random(_) => None,
        }
    }
}

/// Generates one 32-byte master key from the operating system CSPRNG.
///
/// # Errors
/// Returns [`KeyError::Random`] if the operating system cannot provide
/// cryptographically secure randomness.
pub fn generate_key() -> Result<Vec<u8>, KeyError> {
    let mut key = vec![0; KEY_SIZE];
    getrandom::fill(&mut key).map_err(|error| KeyError::Random(error.to_string()))?;
    Ok(key)
}

fn data_dir(xdg_data_home: Option<&Path>, home: Option<&Path>, temp: &Path) -> PathBuf {
    if let Some(path) = xdg_data_home.filter(|path| !path.as_os_str().is_empty()) {
        return path.join("symguard");
    }
    home.map_or_else(
        || temp.join("symguard"),
        |path| path.join(".local/share/symguard"),
    )
}

/// Returns the XDG data path for the capability key.
///
/// The lookup is `$XDG_DATA_HOME/symguard/capability.key`, then the platform
/// home directory's `.local/share/symguard/capability.key`, and finally a
/// temporary-directory fallback when no home directory is available.
#[must_use]
pub fn default_key_path() -> PathBuf {
    let xdg = env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    #[cfg(windows)]
    let home = env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from);
    #[cfg(not(windows))]
    let home = env::var_os("HOME").map(PathBuf::from);
    data_dir(xdg.as_deref(), home.as_deref(), &env::temp_dir()).join("capability.key")
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> KeyError {
    KeyError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

/// Loads existing key material without generating a replacement.
///
/// # Errors
/// Returns an I/O error for missing/unreadable files and
/// [`KeyError::NoKeyMaterial`] for files shorter than 32 bytes.
pub fn load_key(path: impl AsRef<Path>) -> Result<Vec<u8>, KeyError> {
    let path = path.as_ref();
    let key = fs::read(path).map_err(|error| io_error("load", path, error))?;
    if key.len() < KEY_SIZE {
        return Err(KeyError::NoKeyMaterial {
            path: path.to_path_buf(),
            length: key.len(),
        });
    }
    Ok(key)
}

/// Loads an existing key or creates a new 32-byte key on first use.
///
/// Existing bytes are returned unchanged. A present but short key and every
/// write/permission failure fail closed; an existing corrupt file is never
/// silently replaced.
///
/// # Errors
/// Returns [`KeyError`] when the key cannot be loaded, generated, written, or
/// secured with owner-only permissions.
pub fn load_or_create_key(path: impl AsRef<Path>) -> Result<Vec<u8>, KeyError> {
    let path = path.as_ref();
    match fs::read(path) {
        Ok(key) => {
            if key.len() < KEY_SIZE {
                return Err(KeyError::NoKeyMaterial {
                    path: path.to_path_buf(),
                    length: key.len(),
                });
            }
            return Ok(key);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error("load", path, error)),
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| io_error("create directory", parent, error))?;
    }
    let key = generate_key()?;
    fs::write(path, &key).map_err(|error| io_error("write", path, error))?;
    set_owner_only_permissions(path)?;
    Ok(key)
}

fn set_owner_only_permissions(path: &Path) -> Result<(), KeyError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)
            .map_err(|error| io_error("stat", path, error))?
            .permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(path, permissions).map_err(|error| io_error("chmod", path, error))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn generate_key_has_required_size() {
        let first = generate_key().expect("CSPRNG available");
        let second = generate_key().expect("CSPRNG available");
        assert_eq!(first.len(), KEY_SIZE);
        assert_ne!(first, second);
    }

    #[test]
    fn default_data_path_prefers_xdg_then_home_then_temp() {
        let temp = Path::new("/tmp");
        assert_eq!(
            data_dir(
                Some(Path::new("/xdg/data")),
                Some(Path::new("/home/test")),
                temp
            ),
            PathBuf::from("/xdg/data/symguard")
        );
        assert_eq!(
            data_dir(None, Some(Path::new("/home/test")), temp),
            PathBuf::from("/home/test/.local/share/symguard")
        );
        assert_eq!(data_dir(None, None, temp), PathBuf::from("/tmp/symguard"));
    }

    #[test]
    fn load_or_create_is_stable_and_owner_only() {
        let path = std::env::temp_dir()
            .join(format!(
                "symbrain-key-storage-{}-{}",
                std::process::id(),
                unique_test_suffix()
            ))
            .join("symguard/capability.key");
        let first = load_or_create_key(&path).expect("create key");
        let second = load_or_create_key(&path).expect("load key");
        assert_eq!(first, second);
        assert_eq!(first.len(), KEY_SIZE);
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(&path)
                .expect("key metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::remove_dir_all(path.parent().and_then(Path::parent).expect("test parent"))
            .expect("cleanup");
    }

    #[test]
    fn short_existing_key_fails_closed() {
        let dir = std::env::temp_dir().join(format!("symbrain-key-short-{}", unique_test_suffix()));
        fs::create_dir_all(&dir).expect("create test directory");
        let path = dir.join("short.key");
        fs::write(&path, b"short").expect("write short key");
        assert!(matches!(
            load_or_create_key(&path),
            Err(KeyError::NoKeyMaterial { .. })
        ));
        fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn regular_file_parent_fails_closed() {
        let dir =
            std::env::temp_dir().join(format!("symbrain-key-blocker-{}", unique_test_suffix()));
        fs::create_dir_all(&dir).expect("create test directory");
        let blocker = dir.join("blocker");
        fs::write(&blocker, b"x").expect("write blocker");
        assert!(load_or_create_key(blocker.join("key")).is_err());
        fs::remove_dir_all(dir).expect("cleanup");
    }

    fn unique_test_suffix() -> String {
        std::process::id().to_string()
    }
}
