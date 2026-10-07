//! Managed Symaira release manifest, verification, secure extraction, and atomic install.

#![deny(unsafe_code)]

mod archive;
mod install;
mod json_record;
mod local_install;
mod manifest;
mod mkdir;
mod process_status;
mod provenance;
mod version_probe;

pub use archive::{
    atomic_install, extract_binary, find_checksum, safe_archive_path, sha256_file, verify_checksum,
};
pub use install::{InstallOutcome, Installer, versions_match};
pub use local_install::{SourceOrigin, install_source};
pub use manifest::{
    COSIGN_OIDC_ISSUER, Core, ManagedError, Manifest, Platform, download_url, normalize_version,
};
pub use process_status::format as format_process_exit_status;
pub use provenance::go_io_error as format_io_error;
pub use provenance::{
    SourceRecord, is_brain_source_install, read_provenance, read_provenance_bytes,
};
pub use symbrain_core::{GoText, go_json_string_bytes};

pub use version_probe::installed_version;
