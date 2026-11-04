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
//! `SYMAIRA_CUTOVER_BACKUP=off` disables the mechanism for users who manage
//! their own backups; debug builds need `=on` (see [`disabled`]). Caches are
//! never copied.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

#[cfg(unix)]
mod copy;

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
/// Returns an error when an existing root cannot be copied or verified; the
/// partial backup is removed and no marker is written, so the next run retries.
pub fn ensure(app: &str, roots: &[Root], store: &Path) -> io::Result<Outcome> {
    let marker = store.join(format!("{app}.json"));
    if fs::symlink_metadata(&marker).is_ok() {
        return Ok(Outcome::AlreadyDone);
    }
    let present: Vec<&Root> = roots.iter().filter(|root| root.path.is_dir()).collect();
    if present.is_empty() {
        create_private_dir(store)?;
        write_atomic(
            &marker,
            &serde_json::json!({"schema": 1, "app": app, "created_at": utc_now(), "state": "none"}),
        )?;
        return Ok(Outcome::NoState);
    }
    create_private_dir(store)?;
    let stamp = utc_stamp();
    let final_dir = store.join(format!("{app}-{stamp}-{}", std::process::id()));
    let partial = store.join(format!(".{app}-{stamp}-{}.partial", std::process::id()));
    let result = backup_into(app, &present, &partial).and_then(|(files, bytes)| {
        verify_tree(&partial)?;
        fs::rename(&partial, &final_dir)?;
        write_atomic(
            &marker,
            &serde_json::json!({
                "schema": 1,
                "app": app,
                "created_at": utc_now(),
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
            let _ = fs::remove_dir_all(&partial);
            Err(error)
        }
    }
}

/// Runs [`ensure`] for a binary entrypoint and reports on stderr.
///
/// Returns `false` when the backup failed; the caller must then stop before
/// touching state. Success, skip and disabled states return `true`.
pub fn run_for_binary(app: &str, roots: impl FnOnce(&Homes) -> Vec<Root>) -> bool {
    if disabled() {
        return true;
    }
    let Some(homes) = Homes::from_env() else {
        return true;
    };
    let roots = roots(&homes);
    match ensure(app, &roots, &homes.store()) {
        Ok(Outcome::Created { dir, files, .. }) => {
            let _ = writeln!(
                io::stderr(),
                "{app}: backed up existing state ({files} files) to {} before the first Rust run",
                dir.display()
            );
            true
        }
        Ok(_) => true,
        Err(error) => {
            let _ = writeln!(
                io::stderr(),
                "{app}: automatic state backup before the first Rust run failed: {error}\n\
                 {app}: nothing was changed; free space or fix permissions and retry, or set {DISABLE_ENV}=off to skip the backup"
            );
            false
        }
    }
}

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
        #[cfg(unix)]
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

#[cfg(unix)]
fn backup_into(app: &str, roots: &[&Root], partial: &Path) -> io::Result<(u64, u64)> {
    create_private_dir(partial)?;
    let mut entries = Vec::new();
    let mut totals = (0_u64, 0_u64);
    for root in roots {
        let dest = partial.join(root.label);
        copy::copy_root(&root.path, &dest, root.label, &mut entries, &mut totals)?;
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
        "created_at": utc_now(),
        "restore": "cp -Rp <backup>/<label>/. <path>; the manifest records <path> per label",
        "roots": roots_json,
        "entries": entries,
    });
    write_atomic(&partial.join(MANIFEST), &manifest)?;
    Ok(totals)
}

#[cfg(not(unix))]
fn backup_into(_app: &str, _roots: &[&Root], _partial: &Path) -> io::Result<(u64, u64)> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "automatic state backup is implemented for macOS and Linux only",
    ))
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
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        hex_bytes(value.as_bytes())
    }
    #[cfg(not(unix))]
    {
        hex_bytes(value.to_string_lossy().as_bytes())
    }
}

pub(crate) fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
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
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(PathBuf::from(OsString::from_vec(bytes)))
    }
    #[cfg(not(unix))]
    {
        Ok(PathBuf::from(OsString::from(
            String::from_utf8_lossy(&bytes).into_owned(),
        )))
    }
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
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
    fs::rename(&tmp, path)
}

fn utc_parts() -> (i64, u32, u32, u64) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    // Howard Hinnant's days-to-civil conversion.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day, secs % 86_400)
}

fn utc_stamp() -> String {
    let (year, month, day, secs) = utc_parts();
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

fn utc_now() -> String {
    let (year, month, day, secs) = utc_parts();
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests;
