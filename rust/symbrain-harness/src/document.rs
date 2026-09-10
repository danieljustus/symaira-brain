use std::io::Read;
use std::path::Path;

use crate::json::{self, Object, Value};
use crate::toml_backend;
use crate::{Entry, Format, Harness, HarnessError, ServerInfo};

/// A parsed harness configuration that preserves backend-specific structure.
pub struct Document {
    backend: Backend,
}

enum Backend {
    Json {
        root: Value,
        servers_key: String,
    },
    Toml {
        document: toml_edit::DocumentMut,
        servers_key: String,
    },
}

/// Returns an empty document for the harness format.
#[must_use]
pub fn empty(harness: &Harness) -> Document {
    let key = harness.server_key().unwrap_or_default().to_owned();
    let backend = match harness.format {
        Format::Toml => Backend::Toml {
            document: toml_edit::DocumentMut::new(),
            servers_key: key,
        },
        Format::Json | Format::Unsupported => Backend::Json {
            root: Value::Object(Object::new()),
            servers_key: key,
        },
    };
    Document { backend }
}

/// Parses a harness configuration.
///
/// # Errors
/// Returns an error for malformed input or unsupported harnesses.
pub fn parse(harness: &Harness, data: &[u8]) -> Result<Document, HarnessError> {
    let key = harness
        .server_key()
        .ok_or_else(|| {
            HarnessError::Unsupported("harness does not support MCP configuration".into())
        })?
        .to_owned();
    if data.len() > crate::write::MAX_CONFIG_BYTES {
        return Err(match harness.format {
            Format::Json => HarnessError::Json("input exceeds maximum configuration size".into()),
            Format::Toml => HarnessError::Toml("input exceeds maximum configuration size".into()),
            Format::Unsupported => {
                HarnessError::Unsupported("harness does not support MCP configuration".into())
            }
        });
    }
    let backend = match harness.format {
        Format::Json => Backend::Json {
            root: json::parse(data)?,
            servers_key: key,
        },
        Format::Toml => Backend::Toml {
            document: std::str::from_utf8(data)
                .map_err(|_| HarnessError::Toml("input is not UTF-8".into()))?
                .parse::<toml_edit::DocumentMut>()?,
            servers_key: key,
        },
        Format::Unsupported => {
            return Err(HarnessError::Unsupported(
                "harness does not support MCP configuration".into(),
            ));
        }
    };
    Ok(Document { backend })
}

/// Loads and parses a harness configuration file.
///
/// # Errors
/// Returns an I/O or parse error.
pub fn load(harness: &Harness, path: &Path) -> Result<Document, HarnessError> {
    let file = std::fs::File::open(path)?;
    let mut data = Vec::with_capacity(crate::write::MAX_CONFIG_BYTES.min(64 * 1024));
    file.take((crate::write::MAX_CONFIG_BYTES + 1) as u64)
        .read_to_end(&mut data)?;
    parse(harness, &data)
}

impl Document {
    #[must_use]
    /// Returns one stdio server entry when it can be represented as [`Entry`].
    pub fn server(&self, name: &str) -> Option<Entry> {
        match &self.backend {
            Backend::Json { root, servers_key } => json::server_map(root, servers_key)?
                .get(name)
                .and_then(json::entry),
            Backend::Toml {
                document,
                servers_key,
            } => toml_backend::server(document, servers_key, name).and_then(toml_backend::entry),
        }
    }

    #[must_use]
    /// Returns redacted transport metadata for a configured server.
    pub fn server_info(&self, name: &str) -> Option<ServerInfo> {
        match &self.backend {
            Backend::Json { root, servers_key } => json::server_map(root, servers_key)?
                .get(name)
                .and_then(|value| json::server_info(name, value)),
            Backend::Toml {
                document,
                servers_key,
            } => toml_backend::server(document, servers_key, name)
                .and_then(|item| toml_backend::server_info(name, item)),
        }
    }

    #[must_use]
    /// Returns configured server names in deterministic lexical order.
    pub fn server_names(&self) -> Vec<String> {
        let mut names = match &self.backend {
            Backend::Json { root, servers_key } => {
                json::server_map(root, servers_key).map_or_else(Vec::new, |map| map.keys.clone())
            }
            Backend::Toml {
                document,
                servers_key,
            } => toml_backend::server_names(document, servers_key),
        };
        names.sort();
        names
    }

    /// Inserts or replaces a stdio server entry without disturbing other keys.
    pub fn set_server(&mut self, name: &str, entry: Entry) {
        match &mut self.backend {
            Backend::Json { root, servers_key } => {
                json::ensure_server_map(root, servers_key);
                if let Some(map) = json::server_map_mut(root, servers_key) {
                    map.set(name, json::entry_value(entry));
                }
            }
            Backend::Toml {
                document,
                servers_key,
            } => toml_backend::set_server(document, servers_key, name, entry),
        }
    }

    /// Removes a server and reports whether an entry existed.
    pub fn remove_server(&mut self, name: &str) -> bool {
        match &mut self.backend {
            Backend::Json { root, servers_key } => {
                let removed =
                    json::server_map_mut(root, servers_key).is_some_and(|map| map.remove(name));
                json::remove_empty_server_map(root, servers_key, removed);
                removed
            }
            Backend::Toml {
                document,
                servers_key,
            } => toml_backend::remove_server(document, servers_key, name),
        }
    }

    /// Serializes the document using the backend's contract.
    ///
    /// # Errors
    /// Returns an encoding error if the backend cannot serialize its contents.
    pub fn marshal(&self) -> Result<Vec<u8>, HarnessError> {
        match &self.backend {
            Backend::Json { root, .. } => Ok(json::marshal(root)),
            Backend::Toml { document, .. } => Ok(document.to_string().into_bytes()),
        }
    }
}
