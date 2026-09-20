//! Per-skill lifecycle metadata behind `symbrain skills list` and the
//! `skills_list` MCP tool.
//!
//! Every field is derived from evidence, never guessed, and degrades to empty
//! when its source is absent:
//!
//! - `created_at`/`modified_at` come from the filesystem (skill directory
//!   mtime and the newest file mtime in the tree).
//! - `last_rendered_at` and `installs` come from the lifecycle event log; when
//!   the log has no record, the per-target install marker is the fallback.
//! - `last_used` is only reported when the filesystem records a read that
//!   happened after the file was written *and* after the install itself
//!   (`atime > mtime` and `atime >= installed_at + 1 min`). Without that
//!   evidence it stays `null` — a wrong "last used" is worse than none.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::install::{OperationEvent, read_events};
use crate::render::default_targets;

const MARKER_FILE: &str = ".symskills.json";

/// Minimum distance between the install and a read for the read to count as
/// usage. The install itself touches the installed copy (marker writes, path
/// resolution) and that must never be reported as harness usage.
const MIN_USAGE_GAP_SECONDS: i64 = 60;

/// One known installation of a skill for a harness target.
#[derive(Debug, Clone, Serialize)]
pub struct Install {
    /// Harness target name.
    pub target: String,
    /// Installed path for that target.
    pub path: String,
    /// Marker installation timestamp, copied verbatim.
    pub installed_at: String,
}

/// Queryable metadata for one library skill. Timestamp fields are RFC3339 UTC
/// strings and are omitted when their source is absent.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Record {
    /// Skill directory mtime.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub created_at: String,
    /// Newest file mtime in the skill tree.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub modified_at: String,
    /// Newest successful render, from the event log or the marker.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_rendered_at: String,
    /// Known installs, always an array, sorted by target.
    pub installs: Vec<Install>,
    /// Last observed read of an installed copy; `null` without evidence.
    pub last_used: Option<String>,
    /// Where `last_used` came from.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_used_source: String,
}

/// Inputs for [`collect`].
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Lifecycle event log; empty disables log-derived fields.
    pub log_path: PathBuf,
    /// Pre-read event log grouped by skill name. Preferred over `log_path` so a
    /// caller listing many skills reads the log once.
    pub events: Option<BTreeMap<String, Vec<OperationEvent>>>,
    /// User home used to resolve the marker fallback.
    pub home_dir: PathBuf,
    /// Project root for project-scope marker lookups.
    pub project_dir: Option<PathBuf>,
    /// `user` (default) or `project`.
    pub scope: String,
}

/// Reads the whole lifecycle log once and groups records by skill name.
///
/// Returns `None` when the log cannot be read; the caller then falls back to a
/// per-skill read, exactly like the Go implementation.
#[must_use]
pub fn read_events_log(log_path: &Path) -> Option<BTreeMap<String, Vec<OperationEvent>>> {
    if log_path.as_os_str().is_empty() {
        return None;
    }
    let events = read_events(log_path, None, None).ok()?;
    let mut grouped: BTreeMap<String, Vec<OperationEvent>> = BTreeMap::new();
    for event in events {
        grouped.entry(event.skill.clone()).or_default().push(event);
    }
    Some(grouped)
}

/// Assembles the metadata record for the skill rooted at `root`.
///
/// This never fails: every evidence source is optional and every error
/// degrades to an empty field.
#[must_use]
pub fn collect(root: &Path, skill_name: &str, options: &Options) -> Record {
    let mut record = Record {
        created_at: directory_mtime(root).unwrap_or_default(),
        modified_at: newest_mtime(root).unwrap_or_default(),
        ..Record::default()
    };

    let mut installs: BTreeMap<String, Install> = BTreeMap::new();
    let mut last_rendered: Option<(i64, u32)> = None;
    for event in skill_events(skill_name, options) {
        let Some(ts) = parse_timestamp(&event.ts) else {
            continue;
        };
        if event.outcome != "ok" {
            continue;
        }
        match event.event.as_str() {
            "render" => {
                if last_rendered.is_none_or(|current| ts > current) {
                    last_rendered = Some(ts);
                }
            }
            "install" | "profile_install" => {
                if event.target.is_empty() {
                    continue;
                }
                let newer = installs
                    .get(&event.target)
                    .and_then(|current| parse_timestamp(&current.installed_at))
                    .is_none_or(|current| ts.0 > current.0);
                if newer {
                    installs.insert(
                        event.target.clone(),
                        Install {
                            target: event.target.clone(),
                            path: event.path.clone(),
                            installed_at: event.ts.clone(),
                        },
                    );
                }
            }
            _ => {}
        }
    }

    collect_markers(skill_name, options, &mut installs, &mut last_rendered);

    if let Some((rendered, _)) = last_rendered {
        record.last_rendered_at = format_seconds(rendered);
    }
    record.installs = installs.into_values().collect();
    if let Some((used, source)) = last_used(&record.installs) {
        record.last_used = Some(format_nanos(used.0, used.1));
        record.last_used_source = source;
    }
    record
}

fn skill_events(skill_name: &str, options: &Options) -> Vec<OperationEvent> {
    if let Some(grouped) = options.events.as_ref() {
        return grouped.get(skill_name).cloned().unwrap_or_default();
    }
    if options.log_path.as_os_str().is_empty() {
        return Vec::new();
    }
    read_events(&options.log_path, Some(skill_name), None).unwrap_or_default()
}

/// Fills targets the event log does not cover from the install markers, and
/// supplies a render time when no render event was recorded.
fn collect_markers(
    skill_name: &str,
    options: &Options,
    installs: &mut BTreeMap<String, Install>,
    last_rendered: &mut Option<(i64, u32)>,
) {
    if options.home_dir.as_os_str().is_empty() && options.project_dir.is_none() {
        return;
    }
    let scope = if options.scope.is_empty() {
        "user"
    } else {
        options.scope.as_str()
    };
    for target in default_targets() {
        if installs.contains_key(&target) {
            continue;
        }
        let Ok(destination) = crate::install::install_path_for(
            &target,
            &options.home_dir,
            options.project_dir.as_deref(),
            scope,
            skill_name,
        ) else {
            continue;
        };
        let Some(marker) = read_marker(&destination) else {
            continue;
        };
        installs.insert(
            target.clone(),
            Install {
                target: target.clone(),
                path: destination.display().to_string(),
                installed_at: marker.installed.clone(),
            },
        );
        if last_rendered.is_none() {
            *last_rendered = file_timestamp(Path::new(&marker.rendered_at))
                .or_else(|| parse_timestamp(&marker.installed));
        }
    }
}

/// The subset of an install marker the metadata record reads.
#[derive(Debug, serde::Deserialize)]
struct Marker {
    #[serde(default)]
    installed: String,
    #[serde(default)]
    rendered_at: String,
}

/// Reads the install marker at `destination`. A marker without an installed
/// timestamp and without a rendered tree is not evidence.
fn read_marker(destination: &Path) -> Option<Marker> {
    let bytes = fs::read(destination.join(MARKER_FILE)).ok()?;
    let marker: Marker = serde_json::from_slice(&bytes).ok()?;
    if marker.installed.is_empty() && marker.rendered_at.is_empty() {
        return None;
    }
    Some(marker)
}

/// Strongest last-used evidence across all installed copies: the installed
/// `SKILL.md` access time where the filesystem records one usefully. Only the
/// skill file is probed, never the directory, because a directory's access
/// time is bumped by mere path resolution — including the marker reads above.
fn last_used(installs: &[Install]) -> Option<((i64, u32), String)> {
    let mut best: Option<(i64, u32)> = None;
    for install in installs {
        if install.path.is_empty() {
            continue;
        }
        let installed_at = parse_timestamp(&install.installed_at).map(|ts| ts.0);
        let skill_file = Path::new(&install.path).join("SKILL.md");
        if let Some(access) = useful_access_time(&skill_file, installed_at)
            && best.is_none_or(|current| access > current)
        {
            best = Some(access);
        }
    }
    best.map(|access| (access, "install_atime".to_owned()))
}

/// Returns the file's access time only when it records a read that happened
/// after the file was last written (`atime > mtime`) and after the install
/// itself (`atime >= installed_at + 1 min`). On relatime/noatime mounts the
/// access time is not a usage signal, so every other case reports nothing
/// rather than a fabricated timestamp.
fn useful_access_time(path: &Path, installed_at: Option<i64>) -> Option<(i64, u32)> {
    let metadata = fs::metadata(path).ok()?;
    let accessed = access_time(&metadata)?;
    let written = modified_time(&metadata)?;
    if accessed <= written {
        return None;
    }
    if let Some(installed) = installed_at
        && accessed.0 < installed + MIN_USAGE_GAP_SECONDS
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

fn file_timestamp(path: &Path) -> Option<(i64, u32)> {
    modified_time(&fs::metadata(path).ok()?)
}

fn file_mtime(path: &Path) -> Option<i64> {
    file_timestamp(path).map(|ts| ts.0)
}

fn directory_mtime(path: &Path) -> Option<String> {
    let seconds = fs::metadata(path).ok()?.modified().ok().map(to_seconds)?;
    Some(format_seconds(seconds))
}

fn newest_mtime(root: &Path) -> Option<String> {
    let newest = newest_file_mtime(root)?;
    Some(format_seconds(newest))
}

fn newest_file_mtime(root: &Path) -> Option<i64> {
    let mut best: Option<i64> = None;
    walk_mtimes(root, &mut best);
    best
}

fn walk_mtimes(directory: &Path, best: &mut Option<i64>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if entry.file_name() == ".git" {
                continue;
            }
            walk_mtimes(&path, best);
            continue;
        }
        if let Some(seconds) = file_mtime(&path) {
            *best = Some(best.map_or(seconds, |current: i64| current.max(seconds)));
        }
    }
}

fn to_seconds(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
        Err(error) => -i64::try_from(error.duration().as_secs()).unwrap_or(i64::MAX),
    }
}

/// Parses an RFC3339 timestamp into whole seconds. Sub-second precision is
/// deliberately dropped for the ordering comparisons this module performs.
fn parse_timestamp(value: &str) -> Option<(i64, u32)> {
    let parsed = chrono::DateTime::parse_from_rfc3339(value).ok()?;
    Some((parsed.timestamp(), parsed.timestamp_subsec_nanos()))
}

/// Formats whole seconds the way Go's `time.RFC3339` does for a UTC time.
fn format_seconds(seconds: i64) -> String {
    let Some(datetime) = chrono::DateTime::from_timestamp(seconds, 0) else {
        return String::new();
    };
    datetime.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Formats a nanosecond timestamp the way Go's `time.RFC3339Nano` does: the
/// fractional part is emitted with trailing zeros trimmed, and omitted
/// entirely when there is no fraction.
fn format_nanos(seconds: i64, nanos: u32) -> String {
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
