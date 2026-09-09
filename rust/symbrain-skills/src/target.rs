//! Canonical skill-render target identity and order.

use std::path::{Path, PathBuf};

use symbrain_harness::SkillTarget;

/// The canonical `OpenCode` target identity.
pub const OPENCODE: &str = "opencode";

/// Static rendering metadata for a registered skill target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetSpec {
    /// Stable target name.
    pub name: &'static str,
    /// Human-readable target name.
    pub display_name: &'static str,
    /// Capabilities verified for the built-in harness runtime.
    pub capabilities: &'static [(&'static str, bool)],
    /// Optional generated metadata file relative to the rendered skill root.
    pub metadata_file: Option<&'static str>,
}

const SUBAGENTS: &[(&str, bool)] = &[("subagents", true)];
const NO_CAPABILITIES: &[(&str, bool)] = &[];

const OPENCODE_SPEC: TargetSpec = TargetSpec {
    name: OPENCODE,
    display_name: "OpenCode",
    capabilities: NO_CAPABILITIES,
    metadata_file: None,
};
const CLAUDE_SPEC: TargetSpec = TargetSpec {
    name: "claude",
    display_name: "Claude Code",
    capabilities: SUBAGENTS,
    metadata_file: None,
};
const CODEX_SPEC: TargetSpec = TargetSpec {
    name: "codex",
    display_name: "Codex",
    capabilities: NO_CAPABILITIES,
    metadata_file: Some("agents/openai.yaml"),
};
const HERMES_SPEC: TargetSpec = TargetSpec {
    name: "hermes",
    display_name: "Hermes",
    capabilities: SUBAGENTS,
    metadata_file: None,
};
const ANTIGRAVITY_SPEC: TargetSpec = TargetSpec {
    name: "antigravity",
    display_name: "Antigravity",
    capabilities: NO_CAPABILITIES,
    metadata_file: None,
};
const OPENCLAW_SPEC: TargetSpec = TargetSpec {
    name: "openclaw",
    display_name: "OpenClaw",
    capabilities: NO_CAPABILITIES,
    metadata_file: None,
};

/// Resolves the installed skill root for a target and scope.
///
/// User roots follow each harness's documented global location. Project roots
/// deliberately use the shared workspace locations from the Go registry.
/// Callers must provide a non-empty project path for project scope.
#[must_use]
pub fn skill_root(
    target_name: &str,
    home: &Path,
    project: Option<&Path>,
    scope: &str,
) -> Option<PathBuf> {
    let project = project.filter(|path| !path.as_os_str().is_empty());
    match (target_name, scope) {
        ("opencode", "project") => project.map(|path| path.join(".opencode/skills")),
        ("claude", "project") => project.map(|path| path.join(".claude/skills")),
        ("codex" | "antigravity" | "openclaw", "project") => {
            project.map(|path| path.join(".agents/skills"))
        }
        ("hermes", "project") => project.map(|path| path.join(".hermes/skills")),
        ("opencode", _) => Some(home.join(".config/opencode/skills")),
        ("claude", _) => Some(home.join(".claude/skills")),
        ("codex", _) => Some(home.join(".agents/skills")),
        ("hermes", _) => Some(home.join(".hermes/skills/symaira")),
        ("antigravity", _) => Some(home.join(".gemini/config/skills")),
        ("openclaw", _) => Some(home.join(".openclaw/skills")),
        _ => None,
    }
}

/// Returns skill targets in canonical harness registry order.
#[must_use]
pub fn target_names() -> Vec<String> {
    symbrain_harness::all()
        .iter()
        .filter_map(|harness| skill_target_name(harness.skill_target))
        .map(str::to_owned)
        .collect()
}

/// Looks up a registered target without maintaining a second target list.
#[must_use]
pub fn lookup(name: &str) -> Option<TargetSpec> {
    symbrain_harness::all()
        .iter()
        .filter_map(|harness| skill_target_name(harness.skill_target))
        .find(|candidate| *candidate == name)
        .map(|candidate| match candidate {
            "opencode" => OPENCODE_SPEC,
            "claude" => CLAUDE_SPEC,
            "codex" => CODEX_SPEC,
            "hermes" => HERMES_SPEC,
            "antigravity" => ANTIGRAVITY_SPEC,
            "openclaw" => OPENCLAW_SPEC,
            _ => unreachable!("skill target filtered from canonical registry"),
        })
}

/// Returns generated metadata for a target, if its harness contract requires it.
///
/// Metadata is deliberately produced at materialization time: it is an output
/// artifact, not a source Markdown resource, and therefore never participates
/// in variant resolution.
#[must_use]
pub(crate) fn generated_metadata(
    target_name: &str,
    name: &str,
    description: &str,
) -> Option<(&'static str, Vec<u8>)> {
    let spec = lookup(target_name)?;
    let path = spec.metadata_file?;
    let content = format!(
        "interface:\n  display_name: {name:?}\n  short_description: {description:?}\npolicy:\n  allow_implicit_invocation: true\n"
    );
    Some((path, content.into_bytes()))
}

fn skill_target_name(target: SkillTarget) -> Option<&'static str> {
    match target {
        SkillTarget::None => None,
        SkillTarget::OpenCode => Some("opencode"),
        SkillTarget::Claude => Some("claude"),
        SkillTarget::Codex => Some("codex"),
        SkillTarget::Hermes => Some("hermes"),
        SkillTarget::Antigravity => Some("antigravity"),
        SkillTarget::OpenClaw => Some("openclaw"),
    }
}
