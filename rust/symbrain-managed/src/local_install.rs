//! Managed publication of explicitly requested local module builds.
use std::path::Path;

use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{ManagedError, atomic_install};

/// Build identity for a Brain-owned optional module, independent of a release publisher.
#[derive(Clone, Copy)]
pub struct SourceOrigin<'a> {
    pub receiver_commit: &'a str,
    pub module_dir: &'a str,
    pub builder: &'a [u8],
}

#[derive(Serialize)]
struct Provenance<'a> {
    binary: &'a str,
    source: &'static str,
    version: &'static str,
    #[serde(skip_serializing_if = "str::is_empty")]
    receiver_commit: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    module_dir: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    builder: Option<Box<serde_json::value::RawValue>>,
    built_at: String,
    binary_sha256: String,
}

/// Publishes source bytes through the existing managed atomic installer.
///
/// As in Go, binary publication precedes sidecar publication: a sidecar failure
/// leaves the installed payload visible and returns an error. The later version
/// handshake never rewrites the source record's historically empty version.
///
/// # Errors
/// Returns an error for invalid names or filesystem/publication failures.
pub fn install_source(
    bin_dir: &Path,
    binary_name: &str,
    binary: &[u8],
    origin: SourceOrigin<'_>,
) -> Result<(), ManagedError> {
    if binary_name.is_empty()
        || binary_name.contains(['/', '\\', ':', '\0'])
        || matches!(binary_name, "." | "..")
    {
        return Err(ManagedError::Context(
            "managed: install local: invalid binary name".into(),
        ));
    }
    crate::mkdir::create_bin_dir(bin_dir)?;
    atomic_install(bin_dir, binary_name, binary)?;
    let record = Provenance {
        binary: binary_name,
        source: "brain-source",
        version: "",
        receiver_commit: origin.receiver_commit,
        module_dir: origin.module_dir,
        builder: if origin.builder.is_empty() {
            None
        } else {
            Some(
                crate::go_json_string_bytes(origin.builder)
                    .map_err(|error| ManagedError::Context(error.to_string()))?,
            )
        },
        built_at: Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true),
        binary_sha256: format!("{:x}", Sha256::digest(binary)),
    };
    crate::provenance::write_record(bin_dir, binary_name, &record).map_err(|error| {
        ManagedError::RawContext(
            error
                .into_go_text()
                .with_prefix(&format!("managed: record provenance for {binary_name}: ")),
        )
    })
}
