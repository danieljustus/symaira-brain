//! Cross-process bounded operation event logging.
#![allow(clippy::collapsible_if, clippy::too_many_arguments)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::core::install_path_for;
use super::destination::{effective_mode, scope_name};
use super::{InstallOptions, InstallResult};
use crate::model::SkillError;
use fs2::FileExt;

/// One best-effort local skill operation-log record.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OperationEvent {
    /// RFC3339 UTC timestamp.
    pub ts: String,
    /// Operation name (`install` or `uninstall`).
    pub event: String,
    /// Skill name.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub skill: String,
    /// Harness target.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub target: String,
    /// Source/frontmatter version when known.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub skill_version: String,
    /// Content hash when known.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_hash: String,
    /// Installation scope.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub scope: String,
    /// Installation mode.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub mode: String,
    /// Affected destination.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
    /// `ok` or `error`.
    pub outcome: String,
    /// Error detail, when applicable.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
    /// Caller identity.
    pub actor: String,
    /// Version of the writer that produced the event.
    pub tool_version: String,
}

/// Maximum size of the current operation-event segment.
pub const EVENT_MAX_BYTES: u64 = 1 << 20;
const EVENT_LOCK_WAIT: Duration = Duration::from_millis(500);
const EVENT_LOCK_POLL: Duration = Duration::from_millis(10);

/// Appends one event using the current UTC timestamp when the event has none.
pub fn record_event(path: Option<&Path>, event: OperationEvent) {
    let timestamp = chrono::Utc::now().to_rfc3339();
    record_event_at(path, event, &timestamp);
}

/// Appends one event with an injectable timestamp.
///
/// Logging remains best effort: serialization, lock, rotation, and write
/// failures are deliberately ignored so an operation cannot be made to fail
/// merely because its local event log is unavailable.
pub fn record_event_at(path: Option<&Path>, mut event: OperationEvent, timestamp: &str) {
    let Some(path) = path else { return };
    if event.ts.is_empty() {
        if timestamp.is_empty() {
            event.ts = chrono::Utc::now().to_rfc3339();
        } else {
            timestamp.clone_into(&mut event.ts);
        }
    }
    let lock_path = event_lock_path(path);
    let Some(lock_parent) = lock_path.parent() else {
        return;
    };
    if fs::create_dir_all(lock_parent).is_err() {
        return;
    }
    let Ok(lock_file) = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
    else {
        return;
    };
    let deadline = Instant::now() + EVENT_LOCK_WAIT;
    loop {
        match lock_file.try_lock_exclusive() {
            Ok(()) => break,
            Err(_) if Instant::now() < deadline => std::thread::sleep(EVENT_LOCK_POLL),
            Err(_) => return,
        }
    }
    if event.tool_version.is_empty() {
        option_env!("SYMBRAIN_VERSION")
            .or(option_env!("CARGO_PKG_VERSION"))
            .unwrap_or("unknown")
            .clone_into(&mut event.tool_version);
    }
    let Ok(mut bytes) = serde_json::to_vec(&event) else {
        return;
    };
    bytes.push(b'\n');
    if bytes.len() as u64 > EVENT_MAX_BYTES {
        return;
    }
    if let Some(parent) = path.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let rotated = rotated_event_path(path);
    if let Ok(metadata) = fs::metadata(path) {
        if metadata.len().saturating_add(bytes.len() as u64) > EVENT_MAX_BYTES {
            let _ = fs::remove_file(&rotated);
            if fs::rename(path, &rotated).is_err() {
                return;
            }
        }
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(&bytes);
    }
}

fn rotated_event_path(path: &Path) -> PathBuf {
    let extension = path.extension().and_then(|value| value.to_str());
    match extension {
        Some(extension) => path.with_file_name(format!(
            "{}.1.{extension}",
            path.file_stem().unwrap_or_default().to_string_lossy()
        )),
        None => path.with_file_name(format!(
            "{}.1",
            path.file_name().unwrap_or_default().to_string_lossy()
        )),
    }
}
pub(crate) fn record_install_outcome(
    target: &str,
    name: &str,
    options: &InstallOptions,
    result: &Result<InstallResult, SkillError>,
) {
    if options.dry_run {
        return;
    }
    let path = install_path_for(
        target,
        &options.home_dir,
        options.project_dir.as_deref(),
        scope_name(options),
        name,
    )
    .unwrap_or_default();
    let (outcome, error) = match result {
        Ok(_) => ("ok", String::new()),
        Err(error) => ("error", error.0.clone()),
    };
    record_event(
        options.events_path.as_deref(),
        event_for(
            "install",
            target,
            name,
            scope_name(options),
            effective_mode(options),
            &path,
            outcome,
            &error,
        ),
    );
}

pub(crate) fn event_for(
    event: &str,
    target: &str,
    name: &str,
    scope: &str,
    mode: &str,
    path: &Path,
    outcome: &str,
    error: &str,
) -> OperationEvent {
    event_for_at(
        &chrono::Utc::now().to_rfc3339(),
        event,
        target,
        name,
        scope,
        mode,
        path,
        outcome,
        error,
    )
}

pub(crate) fn event_for_at(
    timestamp: &str,
    event: &str,
    target: &str,
    name: &str,
    scope: &str,
    mode: &str,
    path: &Path,
    outcome: &str,
    error: &str,
) -> OperationEvent {
    OperationEvent {
        ts: timestamp.to_owned(),
        event: event.to_owned(),
        skill: name.to_owned(),
        target: target.to_owned(),
        skill_version: String::new(),
        source_hash: String::new(),
        scope: scope.to_owned(),
        mode: mode.to_owned(),
        path: path.to_string_lossy().into_owned(),
        outcome: outcome.to_owned(),
        error: error.to_owned(),
        actor: "cli".to_owned(),
        tool_version: String::new(),
    }
}

fn event_lock_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        ".{}.lock",
        path.file_name().unwrap_or_default().to_string_lossy()
    ))
}
