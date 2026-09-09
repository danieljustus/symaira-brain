//! Error types for profile parsing, validation, and policy evaluation.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Errors arising from profile loading, TOML decoding, and schema validation.
#[derive(Debug)]
pub enum ProfileError {
    /// Profile name failed regex validation (`^[a-zA-Z0-9_-]+$`).
    InvalidName { name: String, message: String },
    /// Profile name in an explicit profile file is invalid or missing.
    InvalidFileName {
        name: String,
        path: PathBuf,
        cause: String,
    },
    /// Reading profile file from disk failed.
    ReadFailed { path: PathBuf, source: io::Error },
    /// TOML parsing failed.
    ParseFailed {
        name: Option<String>,
        path: Option<PathBuf>,
        message: String,
    },
    /// Profile `[profile].name` did not match the file basename.
    NameMismatch { expected: String, actual: String },
    /// Server table failed validation (missing transport, invalid mode/access).
    InvalidServer { name: String, message: String },
    /// Listing profiles directory failed.
    ListFailed { message: String, source: io::Error },
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName { name, message } => {
                write!(f, "profile: invalid name \"{name}\": {message}")
            }
            Self::InvalidFileName { name, path, cause } => {
                write!(
                    f,
                    "profile \"{name}\": invalid or missing name in {}: {cause}",
                    path.display()
                )
            }
            Self::ReadFailed { path, source } => {
                write!(
                    f,
                    "profile: failed to read {}: open {}: {}",
                    path.display(),
                    path.display(),
                    go_io_error(source)
                )
            }
            Self::ParseFailed {
                name: Some(n),
                path: Some(p),
                message,
            } => {
                write!(
                    f,
                    "profile \"{n}\": failed to parse TOML in {}: {message}",
                    p.display()
                )
            }
            Self::ParseFailed {
                name: Some(n),
                path: None,
                message,
            } => {
                write!(f, "profile \"{n}\": failed to parse TOML: {message}")
            }
            Self::ParseFailed {
                name: None,
                path: Some(p),
                message,
            } => {
                write!(
                    f,
                    "profile: failed to parse TOML in {}: {message}",
                    p.display()
                )
            }
            Self::ParseFailed {
                name: None,
                path: None,
                message,
            } => {
                write!(f, "profile: failed to parse TOML: {message}")
            }
            Self::NameMismatch { expected, actual } => {
                write!(
                    f,
                    "profile \"{expected}\": name mismatch: profile.name \"{actual}\" does not match filename \"{expected}\""
                )
            }
            Self::InvalidServer { name, message } => {
                write!(f, "profile \"{name}\": invalid servers: {message}")
            }
            Self::ListFailed { message, source } => {
                write!(f, "profile: {message}: {source}")
            }
        }
    }
}

impl std::error::Error for ProfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ReadFailed { source, .. } | Self::ListFailed { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn go_io_error(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::NotFound => "no such file or directory".to_string(),
        io::ErrorKind::PermissionDenied => "permission denied".to_string(),
        io::ErrorKind::AlreadyExists => "file exists".to_string(),
        _ => error.to_string(),
    }
}

impl ProfileError {
    /// Returns the Symaira CLI exit code associated with this error (exit 2 for invalid config/input).
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        symbrain_core::exit::NO_INPUT
    }
}

/// Errors arising from policy evaluation and tool classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    /// Unknown or unsupported server alias for policy evaluation.
    UnknownServerAlias(String),
    /// Mode not recognized for the given server.
    UnknownMode { server: String, mode: String },
    /// No preset tool list exists for the given server and mode combination.
    NoPreset { server: String, mode: String },
    /// Preset evaluation is unsupported for the given server (e.g. skills has no modes).
    PresetUnsupported(String),
    /// Foreign server access class is neither "read" nor "write".
    InvalidAccessClass { server: String, access: String },
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownServerAlias(alias) => {
                write!(f, "policy: unknown server alias \"{alias}\"")
            }
            Self::UnknownMode { server, mode } => {
                write!(f, "policy: unknown mode \"{mode}\" for server \"{server}\"")
            }
            Self::NoPreset { server, mode } => {
                write!(
                    f,
                    "policy: no preset for server \"{server}\" mode \"{mode}\""
                )
            }
            Self::PresetUnsupported(msg) => {
                write!(f, "policy: {msg}")
            }
            Self::InvalidAccessClass { server, access } => {
                write!(
                    f,
                    "policy: invalid access class \"{access}\" for foreign server \"{server}\""
                )
            }
        }
    }
}

impl std::error::Error for PolicyError {}
