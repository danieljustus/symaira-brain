//! Public variant parser data types and contract constants.

use std::collections::BTreeMap;

/// Severity used for render-blocking variant problems.
pub const SEVERITY_ERROR: &str = "error";
/// Public contract constant `SEVERITY_WARNING`.
pub const SEVERITY_WARNING: &str = "warning";
/// Public contract constant `CODE_MARKER_MALFORMED`.
pub const CODE_MARKER_MALFORMED: &str = "variant_marker_malformed";
/// Public contract constant `CODE_BLOCK_ID_INVALID`.
pub const CODE_BLOCK_ID_INVALID: &str = "block_id_invalid";
/// Public contract constant `CODE_BLOCK_NESTED`.
pub const CODE_BLOCK_NESTED: &str = "block_nested";
/// Public contract constant `CODE_BLOCK_UNCLOSED`.
pub const CODE_BLOCK_UNCLOSED: &str = "block_unclosed";
/// Public contract constant `CODE_BLOCK_UNMATCHED_CLOSE`.
pub const CODE_BLOCK_UNMATCHED_CLOSE: &str = "block_unmatched_close";
/// Public contract constant `CODE_BLOCK_CLOSE_MISMATCH`.
pub const CODE_BLOCK_CLOSE_MISMATCH: &str = "block_close_mismatch";
/// Public contract constant `CODE_BLOCK_DUPLICATE_ID`.
pub const CODE_BLOCK_DUPLICATE_ID: &str = "block_duplicate_id";
/// Public contract constant `CODE_TARGET_LIST_EMPTY`.
pub const CODE_TARGET_LIST_EMPTY: &str = "block_target_list_empty";
/// Public contract constant `CODE_TARGET_UNKNOWN`.
pub const CODE_TARGET_UNKNOWN: &str = "block_target_unknown";
/// Public contract constant `CODE_OVERRIDE_UNKNOWN`.
pub const CODE_OVERRIDE_UNKNOWN: &str = "block_override_unknown";
/// Public contract constant `CODE_OVERRIDE_UNUSED`.
pub const CODE_OVERRIDE_UNUSED: &str = "block_override_unused";
/// Public contract constant `CODE_TERM_UNKNOWN`.
pub const CODE_TERM_UNKNOWN: &str = "term_unknown";
/// Public contract constant `CODE_TERM_NAME_INVALID`.
pub const CODE_TERM_NAME_INVALID: &str = "term_name_invalid";
/// Public contract constant `CODE_TERM_DEFAULT_REQUIRED`.
pub const CODE_TERM_DEFAULT_REQUIRED: &str = "term_default_required";
/// Public contract constant `CODE_HARNESS_COUPLING`.
pub const CODE_HARNESS_COUPLING: &str = "harness_coupling";
/// Public contract constant `DEFAULT_KEY`.
pub const DEFAULT_KEY: &str = "default";
/// Public contract constant `BLOCKS_DIR`.
pub const BLOCKS_DIR: &str = "blocks";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Public enumeration `Kind`.
pub enum Kind {
    /// Variant `Block`.
    Block,
    /// Variant `Only`.
    Only,
    /// Variant `Except`.
    Except,
}
#[derive(Debug, Clone, PartialEq, Eq)]
/// Public data type `Problem`.
pub struct Problem {
    /// Field `code`.
    pub code: String,
    /// Field `severity`.
    pub severity: String,
    /// Field `message`.
    pub message: String,
    /// Field `line`.
    pub line: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
/// Public data type `Region`.
pub struct Region {
    /// Field `kind`.
    pub kind: Kind,
    /// Field `id`.
    pub id: String,
    /// Field `targets`.
    pub targets: Vec<String>,
    /// Field `line`.
    pub line: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
/// Public data type `Scan`.
pub struct Scan {
    /// Field `regions`.
    pub regions: Vec<Region>,
    /// Field `block_ids`.
    pub block_ids: Vec<String>,
    /// Field `terms`.
    pub terms: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
/// Public data type `Options`.
pub struct Options {
    /// Field `target`.
    pub target: String,
    /// Field `overrides`.
    pub overrides: BTreeMap<String, String>,
    /// Field `terms`.
    pub terms: BTreeMap<String, BTreeMap<String, String>>,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
/// Public data type `Result`.
pub struct Result {
    /// Field `text`.
    pub text: String,
    /// Field `blocks`.
    pub blocks: Vec<String>,
    /// Field `terms`.
    pub terms: BTreeMap<String, String>,
    /// Field `replaced_bytes`.
    pub replaced_bytes: usize,
    /// Field `source_bytes`.
    pub source_bytes: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
/// Public data type `Mention`.
pub struct Mention {
    /// Field `name`.
    pub name: String,
    /// Field `line`.
    pub line: usize,
}
