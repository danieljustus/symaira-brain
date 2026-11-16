//! Per-target catalog diagnostics behind `symbrain skills doctor --catalog`.
//!
//! The catalog of a target is its rendered library (`<render_dir>/<target>`),
//! the complete set symbrain would hand to that harness. The report is
//! read-only and deterministic: targets follow registry order, skills and
//! findings are sorted, and similarity is integer permille.

use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::catalog_scan::{Duplicate, find_duplicates, roots};
use crate::catalog_similarity::{Lookalike, THRESHOLD_PERMILLE, find_lookalikes};
use crate::harness_profile::{DEFAULT_CONTEXT_WINDOW_TOKENS, profile};
use crate::model::parse_skill_md;
use crate::target;

pub use crate::catalog_similarity::MIN_WORDS as LOOKALIKE_MIN_WORDS;

/// Inputs for a catalog scan.
#[derive(Debug, Clone, Copy)]
pub struct CatalogOptions<'a> {
    /// Home directory used to resolve user and external roots.
    pub home_dir: &'a Path,
    /// Project directory; empty disables project roots.
    pub project_dir: &'a Path,
    /// Directory holding `<target>/<skill>/SKILL.md` render output.
    pub render_dir: &'a Path,
    /// Explicit context window in tokens; `None` uses the conservative default.
    pub context_window_tokens: Option<usize>,
    /// Globally disabled targets, which are not scanned.
    pub disabled_targets: &'a [String],
}

/// Catalog summary of one target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TargetCatalog {
    /// Stable target name.
    pub target: String,
    /// Rendered catalog directory that was counted.
    pub rendered_root: String,
    /// Rendered skills with a readable `SKILL.md`.
    pub skill_count: usize,
    /// Total description characters over those skills.
    pub description_chars: usize,
    /// Skill-list budget in characters, absent when not profiled.
    pub listing_budget_chars: Option<usize>,
    /// Whether `description_chars` exceeds the budget; absent without one.
    pub over_budget: Option<bool>,
    /// Characters above the budget; absent without one.
    pub over_by: Option<usize>,
    /// Harness version the budget was verified against, if any.
    pub budget_harness_version: Option<String>,
    /// Same-named skills across the target's discovery roots.
    pub duplicates: Vec<Duplicate>,
    /// Skill pairs with near-identical descriptions.
    pub lookalikes: Vec<Lookalike>,
}

/// Catalog diagnostics for all enabled targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogReport {
    /// Context window the budgets were computed for.
    pub context_window_tokens: usize,
    /// `flag` when given explicitly, otherwise `default`.
    pub context_window_source: String,
    /// Lookalike threshold, Jaccard permille.
    pub lookalike_threshold_permille: usize,
    /// Descriptions below this many distinct words are not compared.
    pub lookalike_min_words: usize,
    /// One entry per enabled target, in registry order.
    pub targets: Vec<TargetCatalog>,
}

/// Reads `(name, description)` of every rendered skill, sorted by name.
fn rendered_skills(dir: &Path) -> Vec<(String, String)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut skills: Vec<(String, String)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            let raw = fs::read(entry.path().join("SKILL.md")).ok()?;
            let parsed = parse_skill_md(&raw).ok()?;
            Some((name, parsed.frontmatter.description))
        })
        .collect();
    skills.sort();
    skills
}

fn summarize(name: &str, options: &CatalogOptions<'_>, window: usize) -> TargetCatalog {
    let profile = profile(name);
    let rendered = options.render_dir.join(name);
    let skills = rendered_skills(&rendered);
    let description_chars: usize = skills
        .iter()
        .map(|(_, description)| description.chars().count())
        .sum();
    let budget = profile.and_then(|profile| profile.listing_budget);
    let budget_chars = budget.map(|budget| budget.chars(window));
    let project = Some(options.project_dir).filter(|path| !path.as_os_str().is_empty());
    let discovery = roots(name, profile, options.home_dir, project);
    let recursive = profile.is_some_and(|profile| profile.recursive_discovery);
    TargetCatalog {
        target: name.to_owned(),
        rendered_root: rendered.to_string_lossy().into_owned(),
        skill_count: skills.len(),
        description_chars,
        listing_budget_chars: budget_chars,
        over_budget: budget_chars.map(|limit| description_chars > limit),
        over_by: budget_chars.map(|limit| description_chars.saturating_sub(limit)),
        budget_harness_version: budget.map(|budget| budget.verified.harness_version.to_owned()),
        duplicates: find_duplicates(&discovery, recursive),
        lookalikes: find_lookalikes(&skills),
    }
}

/// Builds the catalog report for every enabled target.
#[must_use]
pub fn catalog_report(options: &CatalogOptions<'_>) -> CatalogReport {
    let window = options
        .context_window_tokens
        .unwrap_or(DEFAULT_CONTEXT_WINDOW_TOKENS);
    CatalogReport {
        context_window_tokens: window,
        context_window_source: if options.context_window_tokens.is_some() {
            "flag"
        } else {
            "default"
        }
        .to_owned(),
        lookalike_threshold_permille: THRESHOLD_PERMILLE,
        lookalike_min_words: LOOKALIKE_MIN_WORDS,
        targets: target::enabled_target_names(options.disabled_targets)
            .iter()
            .map(|name| summarize(name, options, window))
            .collect(),
    }
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
