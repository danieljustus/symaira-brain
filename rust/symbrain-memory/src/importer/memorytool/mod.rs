//! Memory-tool importers (ported for #761 from `importer/memorytool` at
//! v0.12.0): exports of other memory products (ChatGPT, Mem0, OpenMemory),
//! the [`smart::SmartImporter`] that stores their facts with content-hash
//! de-duplication, and the [`adapter::SessionAdapter`] that registers one of
//! them in the [`super::registry::Registry`].
//!
//! Every importer receives its search root as an argument (empty means the
//! tool's default below `home`); tests only ever use temp directories.
// Product names (ChatGPT, OpenMemory) are prose, not code items.
#![allow(clippy::doc_markdown)]

pub mod adapter;
pub mod chatgpt;
mod chatgpt_fs;
pub mod mem0;
pub mod openmemory;
pub mod smart;

#[cfg(test)]
mod chatgpt_tests;
#[cfg(test)]
mod tool_tests;

use super::ImportResult;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A discovered export file of an external memory tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRef {
    /// Tool name.
    pub tool: String,
    /// File path.
    pub path: String,
    /// Always `json`.
    pub format: String,
    /// Last modification time.
    pub modified_at: DateTime<Utc>,
}

/// A metadata value (`interface{}` in Go; only strings and numbers occur).
#[derive(Debug, Clone, PartialEq)]
pub enum MetaValue {
    /// Text.
    Str(String),
    /// A JSON number.
    Num(f64),
}

impl MetaValue {
    /// `fmt.Sprint`.
    #[must_use]
    pub fn sprint(&self) -> String {
        match self {
            Self::Str(text) => text.clone(),
            Self::Num(number) => go_float(*number),
        }
    }
}

/// `%v` of a `float64`: shortest round-trip digits, exponent form outside
/// `1e-4 <= |x| < 1e21`.
fn go_float(number: f64) -> String {
    let magnitude = number.abs();
    if magnitude != 0.0 && !(1e-4..1e21).contains(&magnitude) {
        let text = format!("{number:e}");
        return match text.split_once('e') {
            Some((mantissa, exp)) => match exp.strip_prefix('-') {
                Some(digits) => format!("{mantissa}e-{digits:0>2}"),
                None => format!("{mantissa}e+{exp:0>2}"),
            },
            None => text,
        };
    }
    format!("{number}")
}

/// A memory fact imported from an external tool.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolFact {
    /// The memory text.
    pub content: String,
    /// Tool name.
    pub source: String,
    /// When the fact was created.
    pub timestamp: DateTime<Utc>,
    /// Tool-specific context.
    pub metadata: BTreeMap<String, MetaValue>,
}

/// Outcome of a smart import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportSummary {
    /// Facts stored (or that would be, on a dry run).
    pub created: usize,
    /// Always zero (kept for the Go shape).
    pub merged: usize,
    /// Facts whose content hash already exists.
    pub skipped: usize,
}

/// A memory-focused tool whose export files are imported.
pub trait MemoryToolImporter {
    /// Tool name.
    fn name(&self) -> &str;
    /// Export files below `path` (empty: the tool's default location).
    ///
    /// # Errors
    /// A message describing why `path` could not be examined.
    fn discover_exports(&self, path: &str) -> ImportResult<Vec<ExportRef>>;
    /// The facts of one export file.
    ///
    /// # Errors
    /// A message describing why the file could not be read or parsed.
    fn import_export(&self, export: &ExportRef) -> ImportResult<Vec<ToolFact>>;
}

/// `time.Time{}`: what a failed `time.Parse` leaves behind.
pub(crate) fn zero_time() -> DateTime<Utc> {
    NaiveDate::from_ymd_opt(1, 1, 1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map_or(DateTime::<Utc>::MIN_UTC, |n| n.and_utc())
}

/// RFC 3339 text, or the zero time like `ts, _ := time.Parse(...)`.
pub(crate) fn parse_or_zero(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text).map_or_else(|_| zero_time(), |t| t.with_timezone(&Utc))
}

/// `json.Unmarshal` leaves a field untouched on `null`.
pub(crate) fn nullable<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// `<home>/<dir>` with `home` defaulting to `$HOME`.
pub(crate) fn default_root(home: Option<&Path>, dir: &str) -> ImportResult<PathBuf> {
    let home = match home {
        Some(home) => home.to_path_buf(),
        None => super::util::home_dir()?,
    };
    Ok(home.join(dir))
}

/// Shared `DiscoverExports` of Mem0/OpenMemory: a `.json` file, or every
/// `.json` file below a directory.
pub(crate) fn discover_json(tool: &str, root: &Path) -> ImportResult<Vec<ExportRef>> {
    let info = std::fs::metadata(root).map_err(|e| format!("stat {}: {e}", root.display()))?;
    let reference = |path: &Path| {
        super::modified_at(path).map(|modified_at| ExportRef {
            tool: tool.into(),
            path: path.to_string_lossy().into_owned(),
            format: "json".into(),
            modified_at,
        })
    };
    let is_json = |path: &Path| path.to_string_lossy().ends_with(".json");
    if !info.is_dir() {
        return Ok(if is_json(root) {
            reference(root).into_iter().collect()
        } else {
            Vec::new()
        });
    }
    Ok(super::walk_files(root)
        .iter()
        .filter(|p| is_json(p))
        .filter_map(|p| reference(p))
        .collect())
}

/// Reads and parses a plain JSON export.
pub(crate) fn read_json<T: for<'de> Deserialize<'de>>(path: &str) -> ImportResult<T> {
    let data = std::fs::read(path).map_err(|e| format!("failed to read file: {e}"))?;
    serde_json::from_slice(&data).map_err(|e| format!("failed to parse JSON: {e}"))
}
