//! Managed release manifest and platform naming contracts.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

pub const COSIGN_OIDC_ISSUER: &str = "https://token.actions.githubusercontent.com";
const DEFAULT_RELEASE_WORKFLOW: &str = "release.yml";

#[derive(Debug)]
pub enum ManagedError {
    Manifest(String),
    UnsupportedArchitecture(String),
    Checksum(String),
    Archive(String),
    Download(String),
    DownloadNotFound(String),
    Cosign(String),
    Process(String),
    Context(String),
    IoContext(String, std::io::Error),
    Io(std::io::Error),
}

impl fmt::Display for ManagedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(detail) => write!(formatter, "managed: parse manifest: {detail}"),
            Self::UnsupportedArchitecture(arch) => {
                write!(formatter, "managed: unsupported architecture {arch:?}")
            }
            Self::Checksum(detail) => write!(formatter, "checksum: {detail}"),
            Self::Archive(detail) => write!(formatter, "archive: {detail}"),
            Self::Download(detail) | Self::DownloadNotFound(detail) => {
                write!(formatter, "download: {detail}")
            }
            Self::Cosign(detail) => write!(formatter, "cosign: {detail}"),
            Self::Process(detail) => write!(formatter, "managed: {detail}"),
            Self::Context(detail) => formatter.write_str(detail),
            Self::IoContext(context, error) => write!(formatter, "managed: {context}: {error}"),
            Self::Io(error) => write!(formatter, "managed: {error}"),
        }
    }
}

impl std::error::Error for ManagedError {}

impl ManagedError {
    #[must_use]
    pub const fn is_download_not_found(&self) -> bool {
        matches!(self, Self::DownloadNotFound(_))
    }
}

impl From<std::io::Error> for ManagedError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub cores: BTreeMap<String, Core>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Core {
    pub version: String,
    pub repo: String,
    pub binary_name: String,
    pub asset_prefix: String,
    pub has_cosign: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub release_workflow: String,
    #[serde(default)]
    pub sha256: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub platforms: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub asset_arch: String,
}

impl Manifest {
    /// Parses a managed manifest.
    ///
    /// # Errors
    /// Returns a schema error when JSON decoding fails.
    pub fn parse(bytes: &[u8]) -> Result<Self, ManagedError> {
        serde_json::from_slice(bytes).map_err(|error| ManagedError::Manifest(error.to_string()))
    }

    /// Loads the repository's embedded production manifest.
    ///
    /// # Errors
    /// Returns a schema error if the embedded bytes no longer decode.
    pub fn load_embedded() -> Result<Self, ManagedError> {
        Self::parse(include_bytes!("../assets/manifest.json"))
    }
}

impl Core {
    #[must_use]
    pub fn asset_name(&self, os: &str, arch: &str) -> String {
        format!(
            "{}_{}_{}.{}",
            self.asset_prefix,
            strip_v(&self.version),
            self.os_arch_suffix(os, arch),
            archive_extension(os)
        )
    }

    #[must_use]
    pub fn asset_name_alt(&self, os: &str, arch: &str) -> String {
        format!(
            "{}_{}.{}",
            self.binary_name,
            self.os_arch_suffix(os, arch),
            archive_extension(os)
        )
    }

    #[must_use]
    pub fn checksum_asset_name(&self) -> String {
        format!(
            "{}_{}_checksums.txt",
            self.asset_prefix,
            strip_v(&self.version)
        )
    }

    #[must_use]
    pub const fn checksum_asset_name_alt(&self) -> &'static str {
        "checksums.txt"
    }

    #[must_use]
    pub fn supports_platform(&self, os: &str) -> bool {
        self.platforms.is_empty() || self.platforms.iter().any(|platform| platform == os)
    }

    #[must_use]
    pub fn pinned_checksum(&self, asset: &str) -> Option<&str> {
        self.sha256
            .get(asset)
            .map(String::as_str)
            .filter(|sum| !sum.is_empty())
    }

    #[must_use]
    pub fn tag(&self) -> String {
        format!("v{}", strip_v(&self.version))
    }

    #[must_use]
    pub fn certificate_identity(&self) -> String {
        let workflow = if self.release_workflow.is_empty() {
            DEFAULT_RELEASE_WORKFLOW
        } else {
            &self.release_workflow
        };
        format!(
            "https://github.com/{}/.github/workflows/{}@refs/tags/{}",
            self.repo,
            workflow,
            self.tag()
        )
    }

    #[must_use]
    pub const fn certificate_oidc_issuer(&self) -> &'static str {
        COSIGN_OIDC_ISSUER
    }

    #[must_use]
    pub fn binary_path_in_archive(&self, os: &str, arch: &str) -> String {
        if self.asset_prefix.starts_with("symaira-vault") {
            format!(
                "{}_{}_{}/{}",
                self.asset_prefix,
                strip_v(&self.version),
                self.os_arch_suffix(os, arch),
                self.binary_name
            )
        } else {
            self.binary_name.clone()
        }
    }

    fn os_arch_suffix(&self, os: &str, arch: &str) -> String {
        let arch = if self.asset_arch.is_empty() {
            arch
        } else {
            &self.asset_arch
        };
        format!("{os}_{arch}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platform {
    pub os: &'static str,
    pub arch: &'static str,
}

impl Platform {
    /// Maps the Rust target names onto Symaira release naming.
    ///
    /// # Errors
    /// Returns an error for unsupported architectures.
    pub fn current() -> Result<Self, ManagedError> {
        let os = match std::env::consts::OS {
            "macos" => "darwin",
            other => other,
        };
        let arch = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "amd64",
            other => return Err(ManagedError::UnsupportedArchitecture(other.to_string())),
        };
        Ok(Self { os, arch })
    }
}

#[must_use]
pub fn download_url(base: &str, repo: &str, tag: &str, asset: &str) -> String {
    format!("{base}/{repo}/releases/download/{tag}/{asset}")
}

#[must_use]
pub fn normalize_version(version: &str) -> &str {
    version.trim().strip_prefix('v').unwrap_or(version.trim())
}

fn strip_v(version: &str) -> &str {
    version.strip_prefix('v').unwrap_or(version)
}

fn archive_extension(os: &str) -> &'static str {
    if os == "windows" { "zip" } else { "tar.gz" }
}
