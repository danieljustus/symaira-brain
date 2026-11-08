//! Small filesystem and parsing helpers shared by the importers.
//!
//! Importers never resolve a user directory on their own except through
//! [`home_dir`], which reads `$HOME` at discovery time exactly where the Go
//! importers called `os.UserHomeDir`; every constructor accepts an explicit
//! location so tests stay inside a temp dir.

use chrono::{DateTime, Utc};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Modification time of `path` as UTC (`None` when unreadable).
#[must_use]
pub fn modified_at(path: &Path) -> Option<DateTime<Utc>> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .map(DateTime::<Utc>::from)
}

/// `os.UserHomeDir`.
///
/// # Errors
/// The Go-worded message when `$HOME` is unset or empty.
pub fn home_dir() -> Result<PathBuf, String> {
    match std::env::var_os("HOME") {
        Some(home) if !home.is_empty() => Ok(PathBuf::from(home)),
        _ => Err("$HOME is not defined".into()),
    }
}

/// Regular files below `root` (recursive, sorted for determinism). A missing
/// or unreadable root yields an empty list, like `filepath.Walk` callbacks
/// that skip errors. Symlinks are not followed.
#[must_use]
pub fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_file() {
                out.push(entry.path());
            }
        }
    }
    out.sort();
    out
}

/// Final path element as text.
#[must_use]
pub fn base_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Parent directory as text (`filepath.Dir`).
#[must_use]
pub fn dir_name(path: &Path) -> String {
    path.parent()
        .map_or_else(|| ".".into(), |p| p.to_string_lossy().into_owned())
}

/// Lines of `path` the way `bufio.Scanner` yields them: split at `\n`,
/// trailing `\r` dropped, invalid UTF-8 replaced. A line of `max_token` bytes
/// or more stops the scan with Go's error text; the lines before it are still
/// returned.
///
/// # Errors
/// An open/read failure; a too-long line is reported next to the lines.
pub fn scan_lines(path: &Path, max_token: usize) -> std::io::Result<(Vec<String>, Option<String>)> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.read_to_end(&mut bytes)?;
    let mut lines = Vec::new();
    let mut rest = bytes.as_slice();
    while !rest.is_empty() {
        let (line, next) = match rest.iter().position(|b| *b == b'\n') {
            Some(i) => (&rest[..i], &rest[i + 1..]),
            None => (rest, &rest[rest.len()..]),
        };
        if line.len() >= max_token {
            return Ok((lines, Some("bufio.Scanner: token too long".into())));
        }
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        lines.push(String::from_utf8_lossy(line).into_owned());
        rest = next;
    }
    Ok((lines, None))
}

/// RFC 3339 timestamp, or the Go zero time when absent/unparsable.
#[must_use]
pub fn parse_timestamp(s: &str) -> DateTime<Utc> {
    if s.is_empty() {
        return crate::sync_state::zero_time();
    }
    DateTime::parse_from_rfc3339(s).map_or_else(
        |_| crate::sync_state::zero_time(),
        |t| t.with_timezone(&Utc),
    )
}

/// Unix epoch milliseconds as UTC (`time.UnixMilli`).
#[must_use]
pub fn from_millis(ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ms).unwrap_or_else(crate::sync_state::zero_time)
}

/// Decodes a JSON object into `T` the way `json.Unmarshal` into a struct
/// does: only an object (or `null`, the zero value) is accepted. serde would
/// also read a bare array positionally, which Go rejects.
///
/// # Errors
/// The decoder message, or `json: cannot unmarshal ... into Go value`.
pub fn parse_object<T: serde::de::DeserializeOwned + Default>(text: &[u8]) -> Result<T, String> {
    let trimmed = text.trim_ascii_start();
    if trimmed.first() == Some(&b'{') {
        return serde_json::from_slice(trimmed).map_err(|e| e.to_string());
    }
    if trimmed.starts_with(b"null") {
        return Ok(T::default());
    }
    serde_json::from_slice::<serde_json::Value>(trimmed)
        .map_err(|e| e.to_string())
        .and_then(|_| Err("json: cannot unmarshal into Go value of struct type".into()))
}
