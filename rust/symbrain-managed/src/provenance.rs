//! Provenance sidecars for binaries installed from pinned releases.

use std::io::Write;
use std::path::Path;

#[cfg(unix)]
use std::fs;

use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{Core, ManagedError};

#[path = "provenance_json.rs"]
pub(super) mod json;
#[path = "provenance_time.rs"]
mod time;

/// Reports an intentional source install. Setup, like Go, treats unreadable or
/// malformed records as unknown; doctor repair has a separate fail-closed rule.
#[must_use]
pub fn is_brain_source_install(bin_dir: &Path, binary_name: &str) -> bool {
    read_provenance(bin_dir, binary_name)
        .ok()
        .flatten()
        .is_some_and(|record| record.source == "brain-source")
}

#[derive(Debug, Default)]
pub struct SourceRecord {
    pub source: String,
    pub receiver_commit: String,
    pub binary_sha256: String,
}

/// Reads origin information, distinguishing an absent record from corruption.
///
/// # Errors
/// Returns the Go-compatible read or typed JSON error. Repair callers must
/// leave the binary untouched on error unless force-release was explicit.
pub fn read_provenance(bin_dir: &Path, binary_name: &str) -> Result<Option<SourceRecord>, String> {
    let path = bin_dir.join(format!("{binary_name}.provenance.json"));
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            let operation = if error.kind() == std::io::ErrorKind::IsADirectory {
                "read"
            } else {
                "open"
            };
            return Err(format!(
                "managed: read provenance: {operation} {}: {}",
                path.display(),
                go_io_error(&error)
            ));
        }
    };
    decode_source_record(&bytes)
        .map(Some)
        .map_err(|error| format!("managed: parse provenance for {binary_name}: {error}"))
}

fn decode_source_record(bytes: &[u8]) -> Result<SourceRecord, String> {
    let mut record = SourceRecord::default();
    let mut first_error = None;
    for (key, raw) in crate::json_record::fields(bytes, "managed.Provenance")? {
        let folded = crate::json_record::fold(&key);
        match folded.as_str() {
            "binary" | "source" | "version" | "repo" | "receiver_commit" | "module_dir"
            | "builder" | "binary_sha256" => {
                let kind = if folded == "source" {
                    "managed.ProvenanceSource"
                } else {
                    "string"
                };
                match crate::json_record::string(raw, &format!("Provenance.{folded}"), kind) {
                    Ok(Some(value)) => match folded.as_str() {
                        "source" => record.source = value,
                        "receiver_commit" => record.receiver_commit = value,
                        "binary_sha256" => record.binary_sha256 = value,
                        _ => {}
                    },
                    Ok(None) => {}
                    Err(error) => {
                        first_error.get_or_insert(error);
                    }
                }
            }
            "built_at" => time::validate(raw)?,
            _ => {}
        }
    }
    first_error.map_or(Ok(record), Err)
}

/// Formats an operating-system error without Rust's extra numeric suffix.
#[must_use]
pub fn go_io_error(error: &std::io::Error) -> String {
    let text = error.to_string();
    let text = text.split(" (os error ").next().unwrap_or(&text);
    #[cfg(unix)]
    {
        text.to_lowercase()
    }
    #[cfg(not(unix))]
    {
        text.into()
    }
}

#[derive(Serialize)]
struct ReleaseProvenance<'a> {
    binary: &'a str,
    source: &'static str,
    version: &'a str,
    #[serde(skip_serializing_if = "str::is_empty")]
    repo: &'a str,
    built_at: String,
    binary_sha256: String,
}

pub(super) fn record_release_provenance(
    bin_dir: &Path,
    core: &Core,
    binary: &[u8],
) -> Result<(), ManagedError> {
    let provenance = ReleaseProvenance {
        binary: &core.binary_name,
        source: "release",
        version: &core.version,
        repo: &core.repo,
        built_at: Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true),
        binary_sha256: format!("{:x}", Sha256::digest(binary)),
    };
    write_record(bin_dir, &core.binary_name, &provenance)
}

pub(super) fn write_record(
    bin_dir: &Path,
    binary_name: &str,
    provenance: &impl Serialize,
) -> Result<(), ManagedError> {
    let mut data = serde_json::to_vec_pretty(provenance)
        .map_err(|error| ManagedError::Context(format!("managed: marshal provenance: {error}")))?;
    // encoding/json escapes these even when they occur inside toolchain identity strings.
    let text = String::from_utf8(data).map_err(|error| ManagedError::Context(error.to_string()))?;
    data = text
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
        .into_bytes();
    data.push(b'\n');

    let mut temporary = tempfile::Builder::new()
        .prefix(".provenance-")
        .tempfile_in(bin_dir)
        .map_err(|error| {
            ManagedError::Context(format!("managed: create provenance temp: {error}"))
        })?;
    temporary
        .write_all(&data)
        .map_err(|error| ManagedError::Context(format!("managed: write provenance: {error}")))?;
    temporary
        .as_file_mut()
        .sync_all()
        .map_err(|error| ManagedError::Context(format!("managed: sync provenance: {error}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o644))
            .map_err(|error| {
                ManagedError::Context(format!("managed: chmod provenance: {error}"))
            })?;
    }
    let target = bin_dir.join(format!("{binary_name}.provenance.json"));
    temporary.persist(&target).map_err(|error| {
        #[cfg(unix)]
        let detail = if error.error.kind() == std::io::ErrorKind::IsADirectory {
            "file exists".into()
        } else {
            go_io_error(&error.error)
        };
        #[cfg(not(unix))]
        let detail = go_io_error(&error.error);
        ManagedError::Context(format!(
            "managed: rename provenance: rename {} {}: {detail}",
            error.file.path().display(),
            target.display()
        ))
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::Value;

    use super::record_release_provenance;
    use crate::Core;

    #[test]
    fn release_provenance_matches_go_sidecar_contract() {
        let bin_dir = tempfile::tempdir().expect("temporary bin directory");
        let core = Core {
            version: "v1.2.3".to_string(),
            repo: "owner/tool".to_string(),
            binary_name: "symtool".to_string(),
            ..Core::default()
        };
        let binary = b"installed binary payload";

        record_release_provenance(bin_dir.path(), &core, binary).expect("write provenance");

        let path = bin_dir.path().join("symtool.provenance.json");
        let bytes = fs::read(&path).expect("read sidecar");
        assert!(bytes.ends_with(b"\n"), "Go sidecars end with a newline");
        let text = std::str::from_utf8(&bytes).expect("sidecar UTF-8");
        let mut remaining = text;
        for key in [
            "\"binary\"",
            "\"source\"",
            "\"version\"",
            "\"repo\"",
            "\"built_at\"",
            "\"binary_sha256\"",
        ] {
            let index = remaining.find(key).expect("Go-compatible field order");
            remaining = &remaining[index + key.len()..];
        }
        let value: Value = serde_json::from_slice(&bytes).expect("parse sidecar");
        assert_eq!(value.as_object().expect("sidecar object").len(), 6);
        assert_eq!(value["binary"], "symtool");
        assert_eq!(value["source"], "release");
        assert_eq!(value["version"], "v1.2.3");
        assert_eq!(value["repo"], "owner/tool");
        assert!(
            value["built_at"]
                .as_str()
                .is_some_and(|value| value.ends_with('Z'))
        );
        assert_eq!(
            value["binary_sha256"],
            "e145acca56d558b65847ef309f65716afbc714552e7eac95ed0c7ac6371c16c8"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).expect("metadata").permissions().mode() & 0o777,
                0o644
            );
        }
    }
}
