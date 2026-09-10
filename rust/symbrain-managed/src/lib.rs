//! Managed Symaira release manifest, verification, secure extraction, and atomic install.

#![deny(unsafe_code)]

mod archive;
mod install;
mod manifest;

pub use archive::{
    atomic_install, extract_binary, find_checksum, safe_archive_path, sha256_file, verify_checksum,
};
pub use install::{InstallOutcome, Installer, installed_version, versions_match};
pub use manifest::{
    COSIGN_OIDC_ISSUER, Core, ManagedError, Manifest, Platform, download_url, normalize_version,
};
