//! Bundle validation and render-blocking classification.

use std::collections::BTreeMap;
use std::path::Path;

use crate::load::read_bundle_text;
use crate::model::{
    Bundle, Issue, MAX_BODY_LENGTH, MAX_DESCRIPTION_LENGTH, MAX_NAME_LENGTH, MAX_RESOURCE_SIZE,
};
use crate::variant;

fn valid_slug(value: &str, separator: char) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == separator)
        && !value.starts_with(separator)
        && !value.ends_with(separator)
        && !value.contains("--")
}

fn normalize_issue(code: &str, severity: &str, message: impl Into<String>, path: &str) -> Issue {
    Issue {
        code: code.into(),
        severity: severity.into(),
        message: message.into(),
        path: path.into(),
    }
}
fn blocking(code: &str) -> bool {
    matches!(
        code,
        "overlay_reference_missing"
            | variant::CODE_MARKER_MALFORMED
            | variant::CODE_BLOCK_ID_INVALID
            | variant::CODE_BLOCK_NESTED
            | variant::CODE_BLOCK_UNCLOSED
            | variant::CODE_BLOCK_UNMATCHED_CLOSE
            | variant::CODE_BLOCK_CLOSE_MISMATCH
            | variant::CODE_BLOCK_DUPLICATE_ID
            | variant::CODE_TARGET_LIST_EMPTY
            | variant::CODE_OVERRIDE_UNKNOWN
            | variant::CODE_TERM_UNKNOWN
            | variant::CODE_TERM_NAME_INVALID
            | variant::CODE_TERM_DEFAULT_REQUIRED
    )
}

/// Returns whether an issue code refuses rendering.
#[must_use]
pub fn is_render_blocking(code: &str) -> bool {
    blocking(code)
}

/// Built-in harness target names used by the public convenience validator.
pub const DEFAULT_TARGETS: &[&str] = &[
    "opencode",
    "claude",
    "codex",
    "hermes",
    "antigravity",
    "openclaw",
];

/// Validates a bundle against the built-in target registry.
///
/// Call [`validate_with_targets`] when a caller has registered custom targets.
#[must_use]
pub fn validate(bundle: &Bundle) -> Vec<Issue> {
    let targets = DEFAULT_TARGETS
        .iter()
        .map(|target| (*target).to_owned())
        .collect::<Vec<_>>();
    validate_with_targets(bundle, &targets)
}

/// Validates a bundle, including known-target and cross-file variant checks.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "validation keeps issue ordering identical to the Go oracle"
)]
pub fn validate_with_targets(bundle: &Bundle, known_targets: &[String]) -> Vec<Issue> {
    let mut issues = Vec::new();
    let fm = &bundle.frontmatter;
    if fm.name.trim().is_empty() {
        issues.push(normalize_issue(
            "name_required",
            "error",
            "frontmatter name is required",
            "SKILL.md",
        ));
    } else {
        if fm.name.len() > MAX_NAME_LENGTH {
            issues.push(normalize_issue(
                "name_too_long",
                "error",
                format!(
                    "name exceeds maximum length of {MAX_NAME_LENGTH} characters (actual: {})",
                    fm.name.len()
                ),
                "SKILL.md",
            ));
        }
        if !valid_slug(&fm.name, '-') {
            issues.push(normalize_issue("name_format", "error", "name must use lowercase letters, numbers, and single dashes (no consecutive, leading, or trailing dashes)", "SKILL.md"));
        }
        if bundle.root.file_name().and_then(|name| name.to_str()) != Some(fm.name.as_str()) {
            issues.push(normalize_issue(
                "name_dir_mismatch",
                "error",
                format!(
                    "frontmatter name {:?} must match the parent directory name {:?}",
                    fm.name,
                    bundle
                        .root
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("")
                ),
                "SKILL.md",
            ));
        }
    }
    if fm.description.trim().is_empty() {
        issues.push(normalize_issue(
            "description_required",
            "error",
            "frontmatter description is required",
            "SKILL.md",
        ));
    } else if fm.description.len() > MAX_DESCRIPTION_LENGTH {
        issues.push(normalize_issue("description_too_long", "error", format!("frontmatter description exceeds maximum length of {MAX_DESCRIPTION_LENGTH} characters (actual: {})", fm.description.len()), "SKILL.md"));
    }
    if fm.category.trim().is_empty() {
        issues.push(normalize_issue(
            "category_required",
            "warning",
            "frontmatter category is required; reuse an existing category when possible",
            "SKILL.md",
        ));
    }
    if bundle.body.trim().is_empty() {
        issues.push(normalize_issue(
            "body_required",
            "error",
            "SKILL.md body is empty",
            "SKILL.md",
        ));
    } else if bundle.body.len() > MAX_BODY_LENGTH {
        issues.push(normalize_issue(
            "body_too_long",
            "warning",
            format!(
                "SKILL.md body exceeds maximum length of {MAX_BODY_LENGTH} characters (actual: {})",
                bundle.body.len()
            ),
            "SKILL.md",
        ));
    }
    for resource in &bundle.resources {
        if resource.executable {
            issues.push(normalize_issue("resource_executable", "warning", "resource file is executable; install strips the executable bit unless --allow-executable (or the manifest setting) is set", &resource.path));
        }
        if resource.size > MAX_RESOURCE_SIZE {
            issues.push(normalize_issue(
                "resource_too_large",
                "warning",
                format!(
                    "resource exceeds maximum size of {MAX_RESOURCE_SIZE} bytes (actual: {})",
                    resource.size
                ),
                &resource.path,
            ));
        }
    }
    for (target, config) in &bundle.manifest.targets {
        if !config.enabled {
            continue;
        }
        for reference in [&config.prepend, &config.append] {
            if reference.is_empty() {
                continue;
            }
            if let Err(message) = safe_relative_file(bundle, reference) {
                issues.push(normalize_issue(
                    "overlay_reference_missing",
                    "error",
                    message,
                    &format!("symskills.toml:{target}"),
                ));
            }
        }
    }
    let mut paths = bundle.markdown.keys().cloned().collect::<Vec<_>>();
    paths.sort();
    paths.insert(0, "SKILL.md".into());
    let single_target = bundle
        .manifest
        .targets
        .values()
        .filter(|config| config.enabled)
        .count()
        == 1;
    let check_coupling = !known_targets.is_empty() && !single_target;
    let mut definitions = BTreeMap::<String, String>::new();
    let mut source_ids = Vec::new();
    for path in paths {
        let markdown_text;
        let (text, offset) = if path == "SKILL.md" {
            (&bundle.body as &str, bundle.body_line_offset)
        } else {
            markdown_text = String::from_utf8_lossy(&bundle.markdown[&path]);
            (markdown_text.as_ref(), 0)
        };
        if check_coupling {
            for mention in variant::find_mentions(text, known_targets) {
                issues.push(normalize_issue(variant::CODE_HARNESS_COUPLING, "warning", format!("line {}: names harness {:?} outside any symskills:only region; every other target renders this text too", mention.line + offset, mention.name), &path));
            }
        }
        let (scan, problems) = variant::scan_text(text);
        for problem in problems
            .into_iter()
            .chain(variant::check_region_targets(&scan.regions, known_targets))
        {
            issues.push(normalize_issue(
                &problem.code,
                &problem.severity,
                if problem.line > 0 {
                    format!("line {}: {}", problem.line + offset, problem.message)
                } else {
                    problem.message
                },
                &path,
            ));
        }
        for id in scan.block_ids {
            if let Some(previous) = definitions.get(&id) {
                issues.push(normalize_issue(variant::CODE_BLOCK_DUPLICATE_ID, "error", format!("block id {id:?} is already defined in {previous}; ids address a block across the whole skill and must be unique"), &path));
            } else {
                definitions.insert(id.clone(), path.clone());
                source_ids.push(id);
            }
        }
        for term in scan.terms {
            if !bundle.manifest.terms.contains_key(&term) {
                issues.push(normalize_issue(
                    variant::CODE_TERM_UNKNOWN,
                    "error",
                    format!("term {term:?} is referenced but not defined in [terms]"),
                    &path,
                ));
            }
        }
    }
    let mut overrides = BTreeMap::new();
    for (target, blocks) in &bundle.block_overrides {
        overrides.insert(target.clone(), blocks.keys().cloned().collect::<Vec<_>>());
        if bundle
            .manifest
            .targets
            .get(target)
            .is_some_and(|config| !config.enabled)
        {
            issues.push(normalize_issue(variant::CODE_OVERRIDE_UNUSED, "warning", format!("target {target:?} is disabled but ships block overrides; they are never rendered"), &format!("overlays/{target}/blocks")));
        }
    }
    for problem in variant::check_overrides(&source_ids, &overrides) {
        issues.push(normalize_issue(
            &problem.code,
            &problem.severity,
            problem.message,
            "overlays",
        ));
    }
    for problem in variant::check_terms(&bundle.manifest.terms, known_targets) {
        issues.push(normalize_issue(
            &problem.code,
            &problem.severity,
            problem.message,
            "symskills.toml",
        ));
    }
    issues
}

fn safe_relative_file(bundle: &Bundle, reference: &str) -> Result<(), String> {
    let path = Path::new(reference);
    if path.is_absolute() {
        return Err(format!("overlay reference {reference:?} must be relative"));
    }
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(format!(
            "overlay reference {reference:?} escapes skill root"
        ));
    }
    read_bundle_text(
        bundle,
        path,
        &format!("overlay reference {reference:?}"),
        crate::model::MAX_INPUT_SIZE,
    )
    .map(|_| ())
    .map_err(|error| {
        if error.0.contains("No such file or directory") || error.0.contains("not found") {
            // Keep the user-facing diagnostic byte-for-byte with Go's os.Root
            // error on the supported Unix platforms.
            format!(
                "overlay reference {reference:?}: read {reference}: openat {reference}: no such file or directory"
            )
        } else {
            error.to_string()
        }
    })
}
