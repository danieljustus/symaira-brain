//! Policy evaluation, mode presets, tool exposure classification, and default-deny gatekeeping.

pub mod eval;
pub mod foreign;
pub mod identity;
pub mod presets;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

pub use eval::{evaluate, evaluate_preset};
pub use foreign::evaluate_foreign;
pub use identity::identity_parameter;
pub use presets::{known_tools, preset_tools};

/// Classification verdict of one live tool against effective server policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// Tool passes active exposure policy and is present in the child's catalog.
    Exposed,
    /// Tool is known to the server's reference universe but excluded by mode or explicit deny.
    Hidden,
    /// Tool is unrecognized by any maintained preset universe (default-deny).
    Unknown,
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exposed => write!(f, "exposed"),
            Self::Hidden => write!(f, "hidden"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Tool access classification and source for foreign servers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolExposure {
    /// Access class: "read" or "write".
    pub class: String,
    /// Classification source: "`tools_read`", "`tools_write`", "`read_only_hint`", or "`default_write`".
    pub source: String,
}

/// MCP tool annotations used for upstream tool hinting.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolAnnotations {
    /// Human-readable title for the tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Whether the tool is read-only (does not modify state).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    /// Whether the tool is destructive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    /// Whether repeated calls with the same arguments produce identical effects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    /// Whether the tool interacts with external/open-world resources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
}

/// Minimal tool representation required for foreign server policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignTool {
    /// Tool name.
    pub name: String,
    /// Upstream server `readOnlyHint` annotation, if provided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    /// Full tool annotations, if provided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
}

impl ForeignTool {
    /// Creates a new foreign tool with no annotations.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            read_only_hint: None,
            annotations: None,
        }
    }

    /// Sets the `read_only_hint` on this foreign tool.
    #[must_use]
    pub fn with_read_only_hint(mut self, hint: bool) -> Self {
        self.read_only_hint = Some(hint);
        self
    }

    /// Sets full tool annotations on this foreign tool.
    #[must_use]
    pub fn with_annotations(mut self, annotations: ToolAnnotations) -> Self {
        if self.read_only_hint.is_none() {
            self.read_only_hint = annotations.read_only_hint;
        }
        self.annotations = Some(annotations);
        self
    }

    /// Resolves the effective read-only hint, consulting `read_only_hint` and `annotations`.
    #[must_use]
    pub fn effective_read_only_hint(&self) -> Option<bool> {
        self.read_only_hint
            .or_else(|| self.annotations.as_ref().and_then(|a| a.read_only_hint))
    }
}

/// Structured exposure report summarizing tool classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    /// Server alias.
    pub server: String,
    /// Whether the server is enabled.
    pub enabled: bool,
    /// Mode preset (meaningful for vault and memory).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mode: String,
    /// Tools exposed by active policy and present in child.
    pub exposed: Vec<String>,
    /// Tools known to the reference universe but excluded by policy.
    pub hidden: Vec<String>,
    /// Tools unrecognized by reference presets (always an array, including for foreign servers).
    pub unknown: Vec<String>,
    /// Per-tool access classification for foreign servers (`None` for core servers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposures: Option<BTreeMap<String, ToolExposure>>,
}

impl Report {
    /// Reports the classification verdict for `tool`.
    #[must_use]
    pub fn verdict(&self, tool: &str) -> Verdict {
        if self.exposed.iter().any(|t| t == tool) {
            Verdict::Exposed
        } else if self.hidden.iter().any(|t| t == tool) {
            Verdict::Hidden
        } else {
            Verdict::Unknown
        }
    }
}
