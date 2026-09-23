//! Provenance sidecars for binaries installed from pinned releases.

use std::fs;
use std::io::Write;
use std::path::Path;

use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{Core, ManagedError};

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
    let mut data = serde_json::to_vec_pretty(&provenance)
        .map_err(|error| ManagedError::Context(format!("managed: marshal provenance: {error}")))?;
    data.push(b'\n');

    let mut temporary = tempfile::NamedTempFile::new_in(bin_dir).map_err(|error| {
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
    let target = bin_dir.join(format!("{}.provenance.json", core.binary_name));
    temporary.persist(target).map_err(|error| {
        ManagedError::Context(format!("managed: rename provenance: {}", error.error))
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
