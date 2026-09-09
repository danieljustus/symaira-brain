//! Managed release integrity checks, secure archive selection, and atomic install.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

use crate::{Core, ManagedError};

/// Computes a lowercase SHA-256 digest for a file.
///
/// # Errors
/// Returns an error when the file cannot be read.
pub fn sha256_file(path: &Path) -> Result<String, ManagedError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(ManagedError::Io)?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// Verifies a file against a case-insensitive, whitespace-tolerant digest.
///
/// # Errors
/// Returns an error when reading fails or the digest differs.
pub fn verify_checksum(path: &Path, expected: &str) -> Result<(), ManagedError> {
    let actual = sha256_file(path)?;
    if actual != expected.trim().to_ascii_lowercase() {
        return Err(ManagedError::Checksum(format!(
            "mismatch for {}: got {actual}, want {expected}",
            path.display()
        )));
    }
    Ok(())
}

/// Finds an asset digest in GNU/BSD-style checksum text.
///
/// # Errors
/// Returns an error when the asset is absent.
pub fn find_checksum(text: &str, asset: &str) -> Result<String, ManagedError> {
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() >= 2 && fields.last().is_some_and(|name| *name == asset) {
            return Ok(fields[0].to_string());
        }
    }
    Err(ManagedError::Checksum(format!("asset {asset:?} not found")))
}

/// Validates and normalizes a path stored in an archive.
///
/// # Errors
/// Rejects empty, absolute, volume-qualified, or parent-traversing paths.
pub fn safe_archive_path(name: &str) -> Result<PathBuf, ManagedError> {
    let normalized = name.replace('\\', "/");
    if normalized.is_empty() || normalized.contains('\0') || normalized.starts_with('/') {
        return Err(ManagedError::Archive(
            "path must be non-empty and relative".to_string(),
        ));
    }
    if normalized
        .split('/')
        .next()
        .is_some_and(|part| part.contains(':'))
    {
        return Err(ManagedError::Archive(
            "volume-qualified path is not allowed".to_string(),
        ));
    }
    let path = Path::new(&normalized);
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(ManagedError::Archive(
                    "parent traversal is not allowed".to_string(),
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(ManagedError::Archive("path must be relative".to_string()));
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(ManagedError::Archive("invalid archive path".to_string()));
    }
    Ok(clean)
}

/// Extracts exactly the selected binary bytes from tar.gz or zip.
///
/// # Errors
/// Rejects malformed archives, unsafe paths, links, non-regular candidates,
/// duplicate exact entries, ambiguous basename fallbacks, and missing binaries.
pub fn extract_binary(
    archive_path: &Path,
    core: &Core,
    os: &str,
    arch: &str,
) -> Result<Vec<u8>, ManagedError> {
    if archive_path
        .extension()
        .is_some_and(|extension| extension == "zip")
    {
        extract_zip(archive_path, core, os, arch)
    } else {
        extract_tar_gz(archive_path, core, os, arch)
    }
}

fn extract_tar_gz(
    archive_path: &Path,
    core: &Core,
    os: &str,
    arch: &str,
) -> Result<Vec<u8>, ManagedError> {
    let file = File::open(archive_path)?;
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let target = PathBuf::from(core.binary_path_in_archive(os, arch));
    let mut selection = Selection::default();
    for entry in archive
        .entries()
        .map_err(|error| ManagedError::Archive(format!("tar: {error}")))?
    {
        let mut entry = entry.map_err(|error| ManagedError::Archive(format!("tar: {error}")))?;
        let raw_name = String::from_utf8_lossy(entry.path_bytes().as_ref()).to_string();
        let path = safe_archive_path(&raw_name).map_err(|error| {
            ManagedError::Archive(format!("unsafe tar entry {raw_name:?}: {error}"))
        })?;
        let entry_type = entry.header().entry_type();
        if entry_type.is_symlink() || entry_type.is_hard_link() {
            return Err(ManagedError::Archive(format!(
                "unsafe tar link entry {raw_name:?}"
            )));
        }
        let exact = path == target;
        let fallback = path
            .file_name()
            .is_some_and(|name| name == core.binary_name.as_str());
        if exact || fallback {
            if !entry_type.is_file() {
                return Err(ManagedError::Archive(format!(
                    "binary entry {raw_name:?} is not a regular file"
                )));
            }
            let mut data = Vec::new();
            entry
                .read_to_end(&mut data)
                .map_err(|error| ManagedError::Archive(format!("read binary: {error}")))?;
            selection.add(path, data, exact)?;
        }
    }
    selection.finish(&core.binary_name)
}

fn extract_zip(
    archive_path: &Path,
    core: &Core,
    os: &str,
    arch: &str,
) -> Result<Vec<u8>, ManagedError> {
    let file = File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| ManagedError::Archive(format!("zip: {error}")))?;
    let target = PathBuf::from(core.binary_path_in_archive(os, arch));
    let mut selection = Selection::default();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| ManagedError::Archive(format!("zip: {error}")))?;
        let raw_name = entry.name().to_string();
        let path = safe_archive_path(&raw_name).map_err(|error| {
            ManagedError::Archive(format!("unsafe zip entry {raw_name:?}: {error}"))
        })?;
        let mode = entry.unix_mode().unwrap_or(0);
        if mode & 0o170_000 == 0o120_000 {
            return Err(ManagedError::Archive(format!(
                "unsafe zip symlink entry {raw_name:?}"
            )));
        }
        let exact = path == target;
        let fallback = path
            .file_name()
            .is_some_and(|name| name == core.binary_name.as_str());
        if exact || fallback {
            let kind = mode & 0o170_000;
            if entry.is_dir() || (kind != 0 && kind != 0o100_000) {
                return Err(ManagedError::Archive(format!(
                    "binary entry {raw_name:?} is not a regular file"
                )));
            }
            let mut data = Vec::new();
            entry
                .read_to_end(&mut data)
                .map_err(|error| ManagedError::Archive(format!("read binary: {error}")))?;
            selection.add(path, data, exact)?;
        }
    }
    selection.finish(&core.binary_name)
}

#[derive(Default)]
struct Selection {
    exact: Option<Vec<u8>>,
    fallback: Option<(PathBuf, Vec<u8>)>,
}

impl Selection {
    fn add(&mut self, path: PathBuf, data: Vec<u8>, exact: bool) -> Result<(), ManagedError> {
        if exact {
            if self.exact.replace(data).is_some() {
                return Err(ManagedError::Archive(format!(
                    "duplicate binary entry {}",
                    path.display()
                )));
            }
        } else if let Some((previous, _)) = &self.fallback {
            return Err(ManagedError::Archive(format!(
                "ambiguous binary entries {} and {}",
                previous.display(),
                path.display()
            )));
        } else {
            self.fallback = Some((path, data));
        }
        Ok(())
    }

    fn finish(self, binary_name: &str) -> Result<Vec<u8>, ManagedError> {
        self.exact
            .or_else(|| self.fallback.map(|(_, data)| data))
            .ok_or_else(|| ManagedError::Archive(format!("binary {binary_name:?} not found")))
    }
}

/// Atomically installs executable bytes in an existing destination directory.
///
/// # Errors
/// Returns an error when temporary creation, writing, syncing, permissions, or
/// atomic persistence fails.
pub fn atomic_install(bin_dir: &Path, binary_name: &str, data: &[u8]) -> Result<(), ManagedError> {
    let mut temporary = tempfile::NamedTempFile::new_in(bin_dir)?;
    temporary.write_all(data)?;
    temporary.as_file_mut().sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o755))?;
    }
    temporary
        .persist(bin_dir.join(binary_name))
        .map_err(|error| ManagedError::Io(error.error))?;
    Ok(())
}
