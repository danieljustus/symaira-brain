//! Verification of a finished backup and of a restore against its manifest.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::{MANIFEST, hex, invalid, sha256_file};

/// Verifies a backup directory: every manifest entry under `dir/<label>/`
/// must match the recorded type, mode and SHA-256.
///
/// # Errors
/// Returns `InvalidData` naming the first mismatch.
pub fn verify_tree(dir: &Path) -> io::Result<()> {
    let manifest = read_manifest(dir)?;
    verify_entries(&manifest, |label| Ok(dir.join(label)))
}

/// Verifies that the original locations recorded in the manifest of
/// `backup` hold exactly the backed-up bytes again, i.e. that a restore
/// (`cp -Rp <backup>/<label>/. <path>`) succeeded.
///
/// # Errors
/// Returns `InvalidData` naming the first mismatch or an unknown label.
pub fn verify_restore(backup: &Path) -> io::Result<()> {
    let manifest = read_manifest(backup)?;
    let roots = manifest
        .get("roots")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| invalid("manifest: missing roots".into()))?;
    verify_entries(&manifest, |label| {
        roots
            .iter()
            .find(|root| root.get("label").and_then(serde_json::Value::as_str) == Some(label))
            .and_then(|root| root.get("path_hex").and_then(serde_json::Value::as_str))
            .ok_or_else(|| invalid(format!("manifest: unknown label {label}")))
            .and_then(decode_path)
    })
}

fn read_manifest(dir: &Path) -> io::Result<serde_json::Value> {
    serde_json::from_slice(&fs::read(dir.join(MANIFEST))?)
        .map_err(|error| invalid(format!("manifest: {error}")))
}

fn verify_entries(
    manifest: &serde_json::Value,
    base: impl Fn(&str) -> io::Result<PathBuf>,
) -> io::Result<()> {
    let entries = manifest
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| invalid("manifest: missing entries".into()))?;
    for entry in entries {
        let field = |name: &str| {
            entry
                .get(name)
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| invalid(format!("manifest entry without {name}")))
        };
        let rel = decode_path(field("path_hex")?)?;
        let path = if rel.as_os_str().is_empty() {
            base(field("label")?)?
        } else {
            base(field("label")?)?.join(rel)
        };
        let meta = fs::symlink_metadata(&path)?;
        match field("kind")? {
            "dir" if meta.is_dir() => {}
            "symlink" if meta.file_type().is_symlink() => {
                let target = hex(fs::read_link(&path)?.as_os_str());
                if target != field("target_hex")? {
                    return Err(invalid(format!(
                        "symlink target changed: {}",
                        path.display()
                    )));
                }
            }
            "file" if meta.is_file() => {
                if sha256_file(&path)?.0 != field("sha256")? {
                    return Err(invalid(format!("content changed: {}", path.display())));
                }
            }
            kind => {
                return Err(invalid(format!("{} is not a {kind}", path.display())));
            }
        }
        if field("kind")? != "symlink" {
            use std::os::unix::fs::PermissionsExt;
            let mode = entry.get("mode").and_then(serde_json::Value::as_u64);
            if mode != Some(u64::from(meta.permissions().mode() & 0o7777)) {
                return Err(invalid(format!("mode changed: {}", path.display())));
            }
        }
    }
    Ok(())
}

fn decode_path(value: &str) -> io::Result<PathBuf> {
    if value.is_empty() {
        return Ok(PathBuf::new());
    }
    if !value.len().is_multiple_of(2) {
        return Err(invalid("manifest: odd path_hex".into()));
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|error| invalid(format!("manifest: path_hex: {error}")))?;
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(PathBuf::from(OsString::from_vec(bytes)))
    }
}
