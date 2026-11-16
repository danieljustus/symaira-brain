//! Advisory per-target delivery findings over the rendered bundle.
//!
//! Rules run on what each harness actually receives (the output of
//! [`crate::render_target`]), not on the source bundle, so `symskills:only`
//! regions are measured as delivered. Every finding has severity `warning`,
//! and nothing here is reachable from the default validator.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::harness_profile::{Behaviour, HarnessProfile, Limit, profile};
use crate::model::{Bundle, Issue};
use crate::render::{RenderMetadata, render_target};
use crate::target;

/// Hermes `skill_view` result budget exceeded.
pub const CODE_DELIVERY_BUDGET: &str = "harness_delivery_budget";
/// A `SKILL.md` in a subfolder would be registered as a second skill.
pub const CODE_NESTED_MANIFEST: &str = "harness_nested_manifest";
/// `SKILL.md` larger than a shell read returns whole.
pub const CODE_READ_BUDGET: &str = "harness_read_budget";
/// No trigger phrase inside the harness's listing window.
pub const CODE_LISTING_TRIGGER: &str = "harness_listing_trigger";

/// Trigger phrase the listing window must contain (case-insensitive).
pub const TRIGGER_PHRASE: &str = "use when";

/// One advisory finding for one target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// Target the rendered bundle was measured for.
    pub target: String,
    /// Stable rule id; also the advisory issue code.
    pub rule: String,
    /// Always `warning`.
    pub severity: String,
    /// Measured value.
    pub measured: usize,
    /// Limit the value was compared against.
    pub limit: usize,
    /// Unit of `measured` and `limit`.
    pub unit: String,
    /// Harness version the limit was verified against.
    pub harness_version: String,
    /// Date the limit was verified.
    pub verified_on: String,
    /// Path inside the skill the finding points at.
    pub path: String,
    /// Human-readable explanation including the harness-side effect.
    pub message: String,
}

impl Finding {
    /// Converts the finding to the shared issue shape.
    #[must_use]
    pub fn to_issue(&self) -> Issue {
        Issue {
            code: self.rule.clone(),
            severity: self.severity.clone(),
            message: self.message.clone(),
            path: self.path.clone(),
        }
    }
}

/// What one harness receives for one skill.
#[derive(Debug, Clone, Copy)]
pub struct RenderedFacts<'a> {
    /// Rendered `SKILL.md` text.
    pub skill_md: &'a str,
    /// Rendered frontmatter description.
    pub description: &'a str,
    /// Slash-separated paths of every file shipped with the skill.
    pub paths: &'a [String],
}

fn finding(
    profile: &HarnessProfile,
    rule: &str,
    limit: &Limit,
    measured: usize,
    unit: &str,
    path: &str,
    detail: &str,
) -> Finding {
    Finding {
        target: profile.target.to_owned(),
        rule: rule.to_owned(),
        severity: "warning".to_owned(),
        measured,
        limit: limit.value,
        unit: unit.to_owned(),
        harness_version: limit.verified.harness_version.to_owned(),
        verified_on: limit.verified.verified_on.to_owned(),
        path: path.to_owned(),
        message: format!(
            "{}: {detail} (measured {measured} {unit}, limit {} {unit}; {}; verified against {} on {})",
            profile.target,
            limit.value,
            limit.effect,
            limit.verified.harness_version,
            limit.verified.verified_on,
        ),
    }
}

/// JSON-encoded character count of `text`, as a tool result carries it.
#[must_use]
pub fn json_encoded_chars(text: &str) -> usize {
    serde_json::to_string(text).map_or(text.chars().count(), |json| json.chars().count())
}

/// Returns whether the first `window` characters contain the trigger phrase.
#[must_use]
pub fn has_trigger_within(description: &str, window: usize) -> bool {
    let head: String = description.chars().take(window).collect();
    head.to_lowercase().contains(TRIGGER_PHRASE)
}

fn is_nested_manifest(path: &str) -> bool {
    path.rsplit_once('/')
        .is_some_and(|(_, name)| name == "SKILL.md")
}

/// Evaluates every rule of one profile against one rendered skill.
#[must_use]
pub fn evaluate_facts(profile: &HarnessProfile, facts: &RenderedFacts<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    if let Some(limit) = &profile.body_delivery_budget {
        let measured = json_encoded_chars(facts.skill_md);
        if measured > limit.value {
            findings.push(finding(
                profile,
                CODE_DELIVERY_BUDGET,
                limit,
                measured,
                "json_chars",
                "SKILL.md",
                "rendered SKILL.md exceeds the tool-result budget",
            ));
        }
    }
    if let Some(limit) = &profile.read_budget {
        let measured = facts.skill_md.len();
        if measured > limit.value {
            findings.push(finding(
                profile,
                CODE_READ_BUDGET,
                limit,
                measured,
                "bytes",
                "SKILL.md",
                "rendered SKILL.md exceeds the shell-read size",
            ));
        }
    }
    if let Some(limit) = &profile.listing_trigger_window
        && !has_trigger_within(facts.description, limit.value)
    {
        findings.push(finding(
            profile,
            CODE_LISTING_TRIGGER,
            limit,
            facts.description.chars().count(),
            "chars",
            "SKILL.md",
            &format!(
                "description has no {TRIGGER_PHRASE:?} trigger within its first {} characters",
                limit.value
            ),
        ));
    }
    if profile.nested_manifest.behaviour == Behaviour::LoadsAsSkill
        && let Some(verified) = profile.nested_manifest.verified
    {
        let nested: Vec<&str> = facts
            .paths
            .iter()
            .map(String::as_str)
            .filter(|path| is_nested_manifest(path))
            .collect();
        if let Some(first) = nested.first() {
            let limit = Limit {
                value: 0,
                effect: "the nested manifest is registered as a second top-level skill",
                verified,
            };
            findings.push(finding(
                profile,
                CODE_NESTED_MANIFEST,
                &limit,
                nested.len(),
                "nested_manifests",
                first,
                &format!("nested SKILL.md found: {}", nested.join(", ")),
            ));
        }
    }
    findings
}

/// Renders `bundle` for every enabled target and evaluates the profile rules.
///
/// Targets disabled globally or in the bundle manifest, and targets whose
/// render fails, produce no findings: nothing is delivered to them. Findings
/// come in target registry order, then rule order.
#[must_use]
pub fn evaluate_bundle(bundle: &Bundle, disabled_targets: &[String]) -> Vec<Finding> {
    let metadata = RenderMetadata {
        disabled_targets: disabled_targets.to_vec(),
        ..RenderMetadata::default()
    };
    let mut findings = Vec::new();
    for name in target::enabled_target_names(disabled_targets) {
        let Some(profile) = profile(&name) else {
            continue;
        };
        let Ok(rendered) = render_target(bundle, &name, &metadata) else {
            continue;
        };
        let skill_md = String::from_utf8_lossy(&rendered.skill_md).into_owned();
        let paths: Vec<String> = bundle
            .resources
            .iter()
            .map(|resource| resource.path.clone())
            .chain(rendered.files.keys().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        findings.extend(evaluate_facts(
            profile,
            &RenderedFacts {
                skill_md: &skill_md,
                description: &rendered.frontmatter.description,
                paths: &paths,
            },
        ));
    }
    findings
}

#[cfg(test)]
#[path = "harness_findings_tests.rs"]
mod tests;
