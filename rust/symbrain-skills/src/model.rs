//! Public skill data types, frontmatter parsing, and validation.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use cap_std::fs::Dir;
use serde::{Deserialize, Serialize};

/// Public contract constant `MAX_NAME_LENGTH`.
pub const MAX_NAME_LENGTH: usize = 64;
/// Public contract constant `MAX_DESCRIPTION_LENGTH`.
pub const MAX_DESCRIPTION_LENGTH: usize = 1024;
/// Public contract constant `MAX_BODY_LENGTH`.
pub const MAX_BODY_LENGTH: usize = 50_000;
/// Public contract constant `MAX_RESOURCE_SIZE`.
pub const MAX_RESOURCE_SIZE: u64 = 10 * 1024 * 1024;
/// Maximum size of a control document read into memory.
pub const MAX_INPUT_SIZE: u64 = 16 * 1024 * 1024;
/// Maximum size of YAML frontmatter passed to the YAML parser.
pub const MAX_FRONTMATTER_SIZE: usize = 64 * 1024;
/// Maximum number of entries inventoried in one bundle.
pub const MAX_RESOURCE_ENTRIES: usize = 4096;
/// Maximum total size of inventoried bundle resources.
pub const MAX_TOTAL_RESOURCE_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum directory depth visited while loading resources.
pub const MAX_RESOURCE_DEPTH: usize = 32;

/// Errors raised while decoding a skill bundle or its two source documents.
#[derive(Debug, Clone, PartialEq, Eq)]
/// Public data type `SkillError`.
pub struct SkillError(pub String);

impl fmt::Display for SkillError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for SkillError {}

/// YAML metadata from the leading SKILL.md document.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
/// Public data type `Frontmatter`.
pub struct Frontmatter {
    /// Field `name`.
    pub name: String,
    /// Field `description`.
    pub description: String,
    #[serde(default)]
    /// Field `category`.
    pub category: String,
    #[serde(default)]
    /// Field `version`.
    pub version: String,
    #[serde(default)]
    /// Field `author`.
    pub author: String,
    #[serde(default)]
    /// Field `license`.
    pub license: String,
    #[serde(default)]
    /// Field `compatibility`.
    pub compatibility: String,
    #[serde(default)]
    /// Field `platforms`.
    pub platforms: Vec<String>,
    #[serde(default, rename = "required_environment_variables")]
    /// Field `required_environment_variables`.
    pub required_environment_variables: Vec<String>,
    #[serde(default)]
    /// Field `metadata`.
    pub metadata: serde_json::Value,
}

/// The optional TOML manifest accompanying SKILL.md.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
/// Public data type `Manifest`.
pub struct Manifest {
    /// Field `skill`.
    pub skill: ManifestSkill,
    /// Field `targets`.
    pub targets: BTreeMap<String, TargetConfig>,
    #[serde(default)]
    /// Field `terms`.
    pub terms: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
/// Public data type `ManifestSkill`.
pub struct ManifestSkill {
    /// Field `name`.
    pub name: String,
    /// Field `version`.
    pub version: String,
    /// Field `source`.
    pub source: String,
    /// Field `allow_executable`.
    pub allow_executable: bool,
    /// Field `requires`.
    pub requires: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
/// Public data type `TargetConfig`.
pub struct TargetConfig {
    /// Field `enabled`.
    pub enabled: bool,
    /// Field `alias`.
    pub alias: String,
    /// Field `description`.
    pub description: String,
    /// Field `scope`.
    pub scope: String,
    /// Field `category`.
    pub category: String,
    /// Field `prepend`.
    pub prepend: String,
    /// Field `append`.
    pub append: String,
    /// Field `metadata`.
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Public data type `Resource`.
pub struct Resource {
    /// Field `path`.
    pub path: String,
    /// Field `size`.
    pub size: u64,
    /// Field `mode`.
    pub mode: String,
    /// Field `executable`.
    pub executable: bool,
}

/// Loaded bundle state is produced by [`crate::load_bundle`]; the retained
/// capability root is intentionally not constructible by downstream crates.
#[non_exhaustive]
#[derive(Debug, Clone)]
/// Public data type `Bundle`.
pub struct Bundle {
    /// Field `root`.
    pub root: std::path::PathBuf,
    /// Capability rooted at the trusted bundle directory.
    pub(crate) root_cap: Arc<Dir>,
    /// Field `frontmatter`.
    pub frontmatter: Frontmatter,
    /// Field `manifest`.
    pub manifest: Manifest,
    /// Field `body`.
    pub body: String,
    /// Field `resources`.
    pub resources: Vec<Resource>,
    /// Markdown resources outside overlays, preserved as raw bytes like Go strings.
    /// Callers that parse variants should decode UTF-8 explicitly; invalid bytes
    /// are not silently discarded by the loader.
    pub markdown: BTreeMap<String, Vec<u8>>,
    /// Field `block_overrides`.
    pub block_overrides: BTreeMap<String, BTreeMap<String, String>>,
    /// Field `body_line_offset`.
    pub body_line_offset: usize,
}

/// Parsed SKILL.md body and the number of preceding file lines.
#[derive(Debug, Clone, PartialEq)]
/// Public data type `ParsedSkill`.
pub struct ParsedSkill {
    /// Field `frontmatter`.
    pub frontmatter: Frontmatter,
    /// Field `body`.
    pub body: String,
    /// Field `body_line_offset`.
    pub body_line_offset: usize,
}

/// One user-visible validation finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Public data type `Issue`.
pub struct Issue {
    /// Field `code`.
    pub code: String,
    /// Field `severity`.
    pub severity: String,
    /// Field `message`.
    pub message: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    /// Field `path`.
    pub path: String,
}

#[path = "model_parse.rs"]
mod model_parse;
pub(crate) use model_parse::parse_manifest;
pub use model_parse::parse_skill_md;

/// Normalizes category whitespace while preserving spelling.
#[must_use]
/// Public operation `normalize_category`.
pub fn normalize_category(category: &str) -> String {
    category.split_whitespace().collect::<Vec<_>>().join(" ")
}

///
/// # Errors
///
/// Returns an error when the name is empty, too long, or not a valid skill slug.
pub fn validate_skill_name(name: &str) -> Result<(), SkillError> {
    if name.trim().is_empty() {
        return Err(SkillError("skill name is required".into()));
    }
    if name.len() > MAX_NAME_LENGTH {
        return Err(SkillError(format!(
            "skill name {name:?} exceeds maximum length of {MAX_NAME_LENGTH} characters"
        )));
    }
    if !valid_slug(name, '-') {
        return Err(SkillError(format!(
            "skill name {name:?} must be a single lowercase alphanumeric-dash segment without consecutive, leading, or trailing hyphens"
        )));
    }
    Ok(())
}

fn valid_slug(value: &str, separator: char) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == separator)
        && !value.starts_with(separator)
        && !value.ends_with(separator)
        && !value.contains("--")
}
