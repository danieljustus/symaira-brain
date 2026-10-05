//! Filesystem timestamps and Go-compatible lifecycle time formatting.
use super::MIN_USAGE_GAP_SECONDS;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
/// Returns the file's access time only when it records a read that happened
/// after the file was last written (`atime > mtime`) and after the install
/// itself (`atime >= installed_at + 1 min`). On relatime/noatime mounts the
/// access time is not a usage signal, so every other case reports nothing
/// rather than a fabricated timestamp.
pub(super) fn useful_access_time(
    path: &Path,
    installed_at: Option<(i64, u32)>,
) -> Option<(i64, u32)> {
    let metadata = fs::metadata(path).ok()?;
    let accessed = access_time(&metadata)?;
    let written = modified_time(&metadata)?;
    if accessed <= written {
        return None;
    }
    if let Some(installed) = installed_at
        && accessed
            < (
                installed.0.saturating_add(MIN_USAGE_GAP_SECONDS),
                installed.1,
            )
    {
        return None;
    }
    Some(accessed)
}

/// Access timestamp with nanosecond precision where the platform records one.
/// Platforms without a useful access time report `None`, like the Go build tag
/// split does.
#[cfg(unix)]
fn access_time(metadata: &fs::Metadata) -> Option<(i64, u32)> {
    use std::os::unix::fs::MetadataExt;

    let seconds = metadata.atime();
    let nanos = u32::try_from(metadata.atime_nsec()).unwrap_or(0);
    if seconds == 0 && nanos == 0 {
        return None;
    }
    Some((seconds, nanos))
}

#[cfg(not(unix))]
fn access_time(_metadata: &fs::Metadata) -> Option<(i64, u32)> {
    None
}

fn modified_time(metadata: &fs::Metadata) -> Option<(i64, u32)> {
    let modified = metadata.modified().ok()?;
    let duration = modified.duration_since(UNIX_EPOCH).ok()?;
    Some((
        i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
        duration.subsec_nanos(),
    ))
}

pub(super) fn file_timestamp(path: &Path) -> Option<(i64, u32)> {
    modified_time(&fs::metadata(path).ok()?)
}

fn file_mtime(path: &Path) -> Option<i64> {
    file_timestamp(path).map(|ts| ts.0)
}

pub(super) fn directory_mtime(path: &Path) -> Option<String> {
    let seconds = fs::metadata(path).ok()?.modified().ok().map(to_seconds)?;
    Some(format_seconds(seconds))
}

pub(super) fn newest_mtime(root: &Path) -> Option<String> {
    let newest = newest_file_mtime(root)?;
    Some(format_seconds(newest))
}

fn newest_file_mtime(root: &Path) -> Option<i64> {
    let mut best: Option<i64> = None;
    walk_mtimes(root, &mut best).then_some(best).flatten()
}

fn walk_mtimes(root: &Path, best: &mut Option<i64>) -> bool {
    let mut pending = vec![(root.to_path_buf(), 0_usize)];
    let mut entries_seen = 0_usize;
    while let Some((directory, depth)) = pending.pop() {
        if depth > crate::model::MAX_RESOURCE_DEPTH
            || entries_seen >= crate::model::MAX_RESOURCE_ENTRIES
        {
            return false;
        }
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries {
            if entries_seen >= crate::model::MAX_RESOURCE_ENTRIES {
                return false;
            }
            let Ok(entry) = entry else {
                continue;
            };
            entries_seen += 1;
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                if entry.file_name() != ".git" {
                    pending.push((path, depth + 1));
                }
                continue;
            }
            if let Some(seconds) = file_mtime(&path) {
                *best = Some(best.map_or(seconds, |current: i64| current.max(seconds)));
            }
        }
    }
    true
}

fn to_seconds(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
        Err(error) => -i64::try_from(error.duration().as_secs()).unwrap_or(i64::MAX),
    }
}

/// Parses RFC3339 with nanosecond precision for lifecycle ordering.
pub(super) fn parse_timestamp(value: &str) -> Option<(i64, u32)> {
    let parsed = chrono::DateTime::parse_from_rfc3339(value).ok()?;
    Some((parsed.timestamp(), parsed.timestamp_subsec_nanos()))
}

/// Formats whole seconds the way Go's `time.RFC3339` does for a UTC time.
pub(super) fn format_seconds(seconds: i64) -> String {
    let Some(datetime) = chrono::DateTime::from_timestamp(seconds, 0) else {
        return String::new();
    };
    datetime.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Formats a nanosecond timestamp the way Go's `time.RFC3339Nano` does: the
/// fractional part is emitted with trailing zeros trimmed, and omitted
/// entirely when there is no fraction.
pub(super) fn format_nanos(seconds: i64, nanos: u32) -> String {
    let Some(datetime) = chrono::DateTime::from_timestamp(seconds, nanos) else {
        return String::new();
    };
    let base = datetime.format("%Y-%m-%dT%H:%M:%S").to_string();
    if nanos == 0 {
        return format!("{base}Z");
    }
    let fraction = format!("{nanos:09}");
    let trimmed = fraction.trim_end_matches('0');
    format!("{base}.{trimmed}Z")
}
