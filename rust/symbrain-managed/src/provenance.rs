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
mod json;

/// Reports an intentional source install. Setup, like Go, treats unreadable or
/// malformed records as unknown; doctor repair has a separate fail-closed rule.
#[must_use]
pub fn is_brain_source_install(bin_dir: &Path, binary_name: &str) -> bool {
    std::fs::read(bin_dir.join(format!("{binary_name}.provenance.json")))
        .ok()
        .and_then(|bytes| {
            serde_json::from_slice::<SourceRecord>(&json::replace_invalid_strings(&bytes)).ok()
        })
        .is_some_and(|record| record.source == "brain-source")
}

#[derive(Default)]
struct SourceRecord {
    source: String,
}

impl<'de> serde::Deserialize<'de> for SourceRecord {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RecordVisitor;
        impl<'de> serde::de::Visitor<'de> for RecordVisitor {
            type Value = SourceRecord;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("provenance object or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<SourceRecord, E> {
                Ok(SourceRecord::default())
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<SourceRecord, M::Error> {
                let mut record = SourceRecord::default();
                while let Some(key) = map.next_key::<String>()? {
                    let folded = key
                        .replace('\u{017f}', "s")
                        .replace('\u{212a}', "k")
                        .to_ascii_lowercase();
                    match folded.as_str() {
                        "binary" | "source" | "version" | "repo" | "receiver_commit"
                        | "module_dir" | "builder" | "binary_sha256" => {
                            // encoding/json ignores null for plain string fields,
                            // retains duplicate order, and rejects other scalar types.
                            let value = map.next_value::<Option<String>>()?;
                            if folded == "source"
                                && let Some(value) = value
                            {
                                record.source = value;
                            }
                        }
                        "built_at" => {
                            // time.Time's JSON decoder parses the literal token,
                            // rather than JSON-unescaping its timestamp contents.
                            let raw = map.next_value::<Box<serde_json::value::RawValue>>()?;
                            let token = raw.get();
                            if token != "null"
                                && (!token.starts_with('"')
                                    || !token.ends_with('"')
                                    || !json::valid_time(&token[1..token.len() - 1]))
                            {
                                return Err(serde::de::Error::custom(
                                    "invalid provenance built_at",
                                ));
                            }
                        }
                        _ => {
                            map.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(record)
            }
        }
        deserializer.deserialize_any(RecordVisitor)
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
