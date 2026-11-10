//! One-time automatic backup of existing Symaira state on the first Rust run.
//!
//! The Rust binaries replace the Go releases without a Go fallback. Before a
//! Rust binary touches state that an earlier (Go) release may have written,
//! it copies every existing state root once into
//! `$XDG_STATE_HOME/symaira/cutover-backups/<app>-<utc>-<pid>/<label>/` and
//! records a SHA-256 manifest. The copy is verified against the manifest
//! before the per-app marker `<app>.json` is written; later runs skip it.
//!
//! Restoring is a plain directory copy back to the original path that the
//! manifest records (`cp -Rp <backup>/<label>/. <original>`), after which
//! [`verify_restore`] proves the restored bytes match the backup.
//!
//! The backup never holds a command hostage: [`start_for_binary`] runs it on a
//! worker thread, the entrypoint waits at most [`START_GRACE`] (a quick backup
//! still finishes before the command touches state), and [`Backup::finish`]
//! waits at most [`EXIT_GRACE`] before cancelling. Files are not fsynced one by
//! one (that is minutes of `F_FULLFSYNC` on macOS); the manifest and the
//! directories are, once, before the marker is written. A per-app advisory
//! lock keeps concurrent runs apart and tells a live copy from an orphan.
//! A cancelled or killed copy keeps its single `.<app>-resume.partial` tree:
//! the next run reuses every file whose size and mtime still match the source
//! (hashing it again) and copies only the rest, so short commands converge.
//!
//! `SYMAIRA_CUTOVER_BACKUP=off` disables the mechanism for users who manage
//! their own backups; debug builds need `=on` (see [`disabled`]). Caches are
//! never copied.

use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use sha2::{Digest, Sha256};

mod clock;
mod copy;
mod launch;
mod lock;
mod verify;

pub use launch::{Backup, EXIT_GRACE, START_GRACE, run_for_binary, start_for_binary};
pub use verify::{verify_restore, verify_tree};

/// Environment variable that switches the backup `on` or `off` (see [`disabled`]).
pub const DISABLE_ENV: &str = "SYMAIRA_CUTOVER_BACKUP";
/// Manifest file name inside every backup directory.
pub const MANIFEST: &str = "manifest.json";

/// One state directory owned by an application.
#[derive(Debug, Clone)]
pub struct Root {
    /// Stable directory name inside the backup (`config`, `data`, ...).
    pub label: &'static str,
    /// Absolute original location.
    pub path: PathBuf,
}

/// Result of [`ensure`].
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Disabled through [`DISABLE_ENV`] or no usable home directory.
    Disabled,
    /// The marker already exists; nothing was done.
    AlreadyDone,
    /// No state root existed; a marker was written so no later backup runs.
    NoState,
    /// Another live process holds the backup slot; this run did not wait.
    InProgress,
    /// A verified backup was created.
    Created {
        /// Final backup directory.
        dir: PathBuf,
        /// Number of regular files copied.
        files: u64,
        /// Total regular-file bytes copied.
        bytes: u64,
    },
}

/// XDG base directories resolved from the process environment.
#[derive(Debug, Clone)]
pub struct Homes {
    /// `$XDG_CONFIG_HOME` or `~/.config`.
    pub config: PathBuf,
    /// `$XDG_DATA_HOME` or `~/.local/share`.
    pub data: PathBuf,
    /// `$XDG_STATE_HOME` or `~/.local/state`.
    pub state: PathBuf,
}

impl Homes {
    /// Resolves the XDG homes; relative `XDG_*` values are ignored as the
    /// XDG specification requires. Returns `None` without a usable `$HOME`.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let home = std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())?;
        let pick = |name: &str, fallback: &[&str]| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .unwrap_or_else(|| fallback.iter().fold(home.clone(), |p, c| p.join(c)))
        };
        Some(Self {
            config: pick("XDG_CONFIG_HOME", &[".config"]),
            data: pick("XDG_DATA_HOME", &[".local", "share"]),
            state: pick("XDG_STATE_HOME", &[".local", "state"]),
        })
    }

    /// Directory holding every backup and per-app marker.
    #[must_use]
    pub fn store(&self) -> PathBuf {
        self.state.join("symaira").join("cutover-backups")
    }
}

/// True when the mechanism is off for this process.
///
/// Release builds back up by default; `SYMAIRA_CUTOVER_BACKUP=off|0|false|no`
/// turns it off. Debug builds (`cargo test`, `cargo run`) only back up with
/// `SYMAIRA_CUTOVER_BACKUP=on|1|true|yes`, so differential test fixtures that
/// seed Go-era state are not changed by a backup they never asked for.
#[must_use]
pub fn disabled() -> bool {
    let value =
        std::env::var_os(DISABLE_ENV).map(|value| value.to_string_lossy().to_ascii_lowercase());
    match value.as_deref() {
        Some("off" | "0" | "false" | "no") => true,
        Some("on" | "1" | "true" | "yes") => false,
        _ => cfg!(debug_assertions),
    }
}

/// Backs up `roots` once for `app` into `store`.
///
/// # Errors
/// Returns an error when an existing root cannot be copied or verified; no
/// marker is written, so the next run retries. A failed partial backup is
/// removed; a cancelled one (`Interrupted`) is kept and resumed.
pub fn ensure(app: &str, roots: &[Root], store: &Path) -> io::Result<Outcome> {
    ensure_cancellable(app, roots, store, &AtomicBool::new(false))
}

fn marker_exists(marker: &Path) -> bool {
    fs::symlink_metadata(marker).is_ok()
}

/// [`ensure`] that stops with `ErrorKind::Interrupted` once `cancel` is set.
pub(crate) fn ensure_cancellable(
    app: &str,
    roots: &[Root],
    store: &Path,
    cancel: &AtomicBool,
) -> io::Result<Outcome> {
    let marker = store.join(format!("{app}.json"));
    if marker_exists(&marker) {
        return Ok(Outcome::AlreadyDone);
    }
    let present: Vec<&Root> = roots.iter().filter(|root| root.path.is_dir()).collect();
    if present.is_empty() {
        create_private_dir(store)?;
        write_atomic(
            &marker,
            &serde_json::json!({"schema": 1, "app": app, "created_at": clock::utc_now(), "state": "none"}),
        )?;
        return Ok(Outcome::NoState);
    }
    create_private_dir(store)?;
    let Some(_slot) = lock::try_acquire(store, app)? else {
        return Ok(Outcome::InProgress);
    };
    if marker_exists(&marker) {
        return Ok(Outcome::AlreadyDone);
    }
    // Holding the slot proves no live run owns a partial tree: whatever is
    // left belongs to a run that was killed mid-copy.
    lock::remove_orphans(store, app);
    let stamp = clock::utc_stamp();
    let final_dir = store.join(format!("{app}-{stamp}-{}", std::process::id()));
    // One fixed name per app: a cancelled run's copy is resumed by the next run.
    let partial = store.join(format!(".{app}-resume.partial"));
    let result = backup_into(app, &present, &partial, cancel).and_then(|(files, bytes)| {
        verify_tree(&partial)?;
        fs::rename(&partial, &final_dir)?;
        write_atomic(
            &marker,
            &serde_json::json!({
                "schema": 1,
                "app": app,
                "created_at": clock::utc_now(),
                "state": "backed-up",
                "backup": final_dir.to_string_lossy(),
                "files": files,
                "bytes": bytes,
            }),
        )?;
        Ok((files, bytes))
    });
    match result {
        Ok((files, bytes)) => Ok(Outcome::Created {
            dir: final_dir,
            files,
            bytes,
        }),
        Err(error) => {
            // A cancelled copy is kept so the next run resumes it (#1020).
            if error.kind() != io::ErrorKind::Interrupted {
                let _ = fs::remove_dir_all(&partial);
            }
            Err(error)
        }
    }
}

fn backup_into(
    app: &str,
    roots: &[&Root],
    partial: &Path,
    cancel: &AtomicBool,
) -> io::Result<(u64, u64)> {
    create_private_dir(partial)?;
    // A resumed partial may hold a stale manifest, temp file or removed root.
    for child in fs::read_dir(partial)? {
        let child = child?;
        if !roots.iter().any(|root| child.file_name() == root.label) {
            let path = child.path();
            if child.file_type()?.is_dir() {
                fs::remove_dir_all(path)?;
            } else {
                fs::remove_file(path)?;
            }
        }
    }
    let mut walk = copy::Walk::new(cancel);
    for root in roots {
        let dest = partial.join(root.label);
        copy::copy_root(&root.path, &dest, root.label, &mut walk)?;
    }
    let roots_json: Vec<_> = roots
        .iter()
        .map(|root| {
            serde_json::json!({
                "label": root.label,
                "path": root.path.to_string_lossy(),
                "path_hex": hex(root.path.as_os_str()),
            })
        })
        .collect();
    let manifest = serde_json::json!({
        "schema": 1,
        "app": app,
        "created_at": clock::utc_now(),
        "restore": "cp -Rp <backup>/<label>/. <path>; the manifest records <path> per label",
        "roots": roots_json,
        "entries": walk.entries,
    });
    write_atomic(&partial.join(MANIFEST), &manifest)?;
    // The single durability point: files are verified by content below, the
    // directory entries that make them reachable are flushed here.
    copy::sync_dirs(partial)?;
    Ok(walk.totals)
}

pub(crate) fn sha256_file(path: &Path) -> io::Result<(String, u64)> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut size = 0_u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    Ok((hex_bytes(&hasher.finalize()), size))
}

pub(crate) fn hex(value: &std::ffi::OsStr) -> String {
    {
        use std::os::unix::ffi::OsStrExt;
        hex_bytes(value.as_bytes())
    }
}

pub(crate) fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

pub(crate) fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(crate) fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

fn write_atomic(path: &Path, value: &serde_json::Value) -> io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    bytes.push(b'\n');
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file = fs::File::create(&tmp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)?;
    sync_dir(path.parent())
}

/// Flushes a directory entry (best effort where directories cannot be opened).
pub(crate) fn sync_dir(dir: Option<&Path>) -> io::Result<()> {
    match dir {
        Some(dir) => fs::File::open(dir).and_then(|handle| handle.sync_all()),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_background;
