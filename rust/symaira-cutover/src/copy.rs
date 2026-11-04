//! Non-following recursive copy that records a manifest entry per node.
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::Path;
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use crate::{hex, hex_bytes};

/// Attempts per file before a concurrently changing source fails the backup.
const STABLE_ATTEMPTS: usize = 3;

pub(crate) fn copy_root(
    src: &Path,
    dest: &Path,
    label: &str,
    entries: &mut Vec<serde_json::Value>,
    totals: &mut (u64, u64),
) -> io::Result<()> {
    // The root itself may be a symlink (dotfile managers); its target is
    // copied. Links below the root are preserved as links, never followed.
    copy_dir(src, dest, Path::new(""), label, entries, totals)
}

fn copy_dir(
    src: &Path,
    dest: &Path,
    rel: &Path,
    label: &str,
    entries: &mut Vec<serde_json::Value>,
    totals: &mut (u64, u64),
) -> io::Result<()> {
    let mode = fs::metadata(src)?.permissions().mode() & 0o7777;
    fs::DirBuilder::new().mode(0o700).create(dest)?;
    entries.push(entry(label, rel, "dir", mode, None, None));
    let mut children: Vec<_> = fs::read_dir(src)?.collect::<io::Result<_>>()?;
    children.sort_by_key(fs::DirEntry::file_name);
    for child in children {
        let name = child.file_name();
        let (from, to, child_rel) = (src.join(&name), dest.join(&name), rel.join(&name));
        let meta = fs::symlink_metadata(&from)?;
        let kind = meta.file_type();
        if kind.is_dir() {
            copy_dir(&from, &to, &child_rel, label, entries, totals)?;
        } else if kind.is_symlink() {
            let target = fs::read_link(&from)?;
            symlink(&target, &to)?;
            let mut value = entry(label, &child_rel, "symlink", 0, None, None);
            value["target_hex"] = hex(target.as_os_str()).into();
            entries.push(value);
        } else if kind.is_file() {
            let (sha, size) = copy_file(&from, &to)?;
            let file_mode = meta.permissions().mode() & 0o7777;
            fs::set_permissions(&to, fs::Permissions::from_mode(file_mode))?;
            entries.push(entry(
                label,
                &child_rel,
                "file",
                file_mode,
                Some(sha),
                Some(size),
            ));
            totals.0 += 1;
            totals.1 += size;
        }
        // Sockets, FIFOs and devices are runtime endpoints, not state; skipped.
    }
    fs::set_permissions(dest, fs::Permissions::from_mode(mode))
}

fn copy_file(from: &Path, to: &Path) -> io::Result<(String, u64)> {
    for _ in 0..STABLE_ATTEMPTS {
        let before = stamp(from)?;
        let mut source = fs::File::open(from)?;
        let mut target = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(to)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 64 * 1024];
        let mut size = 0_u64;
        loop {
            let read = source.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            target.write_all(&buffer[..read])?;
            size += read as u64;
        }
        target.sync_all()?;
        if stamp(from)? == before && before.0 == size {
            return Ok((hex_bytes(&hasher.finalize()), size));
        }
    }
    Err(io::Error::other(format!(
        "{} kept changing while it was copied; stop the process writing it and retry",
        from.display()
    )))
}

fn stamp(path: &Path) -> io::Result<(u64, Option<SystemTime>)> {
    let meta = fs::metadata(path)?;
    Ok((meta.len(), meta.modified().ok()))
}

fn entry(
    label: &str,
    rel: &Path,
    kind: &str,
    mode: u32,
    sha256: Option<String>,
    size: Option<u64>,
) -> serde_json::Value {
    let mut value = serde_json::json!({
        "label": label,
        "path": rel.to_string_lossy(),
        "path_hex": hex(rel.as_os_str()),
        "kind": kind,
    });
    if kind != "symlink" {
        value["mode"] = mode.into();
    }
    if let Some(sha256) = sha256 {
        value["sha256"] = sha256.into();
    }
    if let Some(size) = size {
        value["size"] = size.into();
    }
    value
}
