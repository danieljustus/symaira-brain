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

use serde::Serialize;

use crate::install::{OperationEvent, read_events};
use crate::render::default_targets;
#[path = "metadata_times.rs"]
mod times;
use times::{
    directory_mtime, file_timestamp, format_nanos, format_seconds, newest_mtime, parse_timestamp,
    useful_access_time,
};

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
                    .is_none_or(|current| ts > current);
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
#[derive(Debug, Default)]
struct Marker {
    installed: String,
    rendered_at: String,
}

/// Reads the install marker at `destination`. A marker without an installed
/// timestamp and without a rendered tree is not evidence.
fn read_marker(destination: &Path) -> Option<Marker> {
    let root =
        cap_std::fs::Dir::open_ambient_dir(destination, ambient_authority::ambient_authority())
            .ok()?;
    let bytes = crate::load::read_limited_nofollow(
        &root,
        Path::new(MARKER_FILE),
        MARKER_FILE,
        crate::model::MAX_INPUT_SIZE,
    )
    .ok()?;
    let marker = decode_marker(&bytes)?;
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
        let installed_at = parse_timestamp(&install.installed_at);
        let skill_file = Path::new(&install.path).join("SKILL.md");
        if let Some(access) = useful_access_time(&skill_file, installed_at)
            && best.is_none_or(|current| access > current)
        {
            best = Some(access);
        }
    }
    best.map(|access| (access, "install_atime".to_owned()))
}

fn decode_marker(raw: &[u8]) -> Option<Marker> {
    let fields = crate::install::marker_json::fields(raw).ok()?;
    if !matches!(raw.trim_ascii().first(), Some(b'{')) {
        return None;
    }
    let mut marker = Marker::default();
    for (key, value) in fields {
        let key: String =
            serde_json::from_str(&crate::install::marker_string::repair_json_strings(key)).ok()?;
        let key: String = key
            .chars()
            .map(|ch| match ch {
                '\u{212a}' => 'k',
                '\u{017f}' => 's',
                _ => ch.to_ascii_lowercase(),
            })
            .collect();
        if !matches!(
            key.as_str(),
            "target" | "name" | "installed" | "rendered_at"
        ) || value.trim_ascii() == b"null"
        {
            continue;
        }
        let value: String =
            serde_json::from_str(&crate::install::marker_string::repair_json_strings(value))
                .ok()?;
        match key.as_str() {
            "installed" => marker.installed = value,
            "rendered_at" => marker.rendered_at = value,
            _ => {}
        }
    }
    Some(marker)
}

#[cfg(test)]
#[path = "metadata_marker_tests.rs"]
mod marker_tests;
