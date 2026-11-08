//! Harness-to-library pull (`symbrain skills pull`).
//!
//! A pull compares one installed skill with a fresh render of its library
//! source, stages only harness-side edits into a pending tree and never writes
//! the library itself. [`apply_pending`] is the explicit promotion step.
//! Overlay-produced body regions and frontmatter keys are refused, not merged.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::install::{self, DriftKind, MARKER_FILE};
use crate::model::SkillError;
use crate::pull_frontmatter::pull_frontmatter;
use crate::pull_helpers::{pull_body, read_skill_md, stage_tree};
use crate::pull_lock::acquire;
use crate::tree_copy::{copy_tree, replace_contents};
use crate::{
    RenderMetadata, load_bundle, materialize, render_target, validate, validate_skill_name, vcs,
};

/// Marker file of a staged pull; never copied into the library.
pub(crate) const MANIFEST_FILE: &str = ".symskills-pull.json";

/// Inputs of one pull or apply.
#[derive(Debug, Clone, Default)]
pub struct PullOptions {
    /// User home directory.
    pub home_dir: PathBuf,
    /// Project directory for project scope.
    pub project_dir: Option<PathBuf>,
    /// `user` or `project`.
    pub scope: String,
    /// Library directory.
    pub library_dir: PathBuf,
    /// Root of the pending trees.
    pub pending_dir: PathBuf,
    /// Base snapshot root override.
    pub base_dir: Option<PathBuf>,
    /// Harness target.
    pub target: String,
    /// Skill name.
    pub name: String,
    /// Plan without staging.
    pub dry_run: bool,
}

/// One content or resource change that would be staged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PullChange {
    /// Slash-separated path inside the skill.
    pub path: String,
    /// `modified`, `added` or `removed`.
    pub status: String,
}

/// A frontmatter key change, kept apart from prose because it can alter
/// harness permissions.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PullFrontmatterChange {
    /// Key, `metadata.<key>` for nested metadata.
    pub key: String,
    /// Library value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<serde_json::Value>,
    /// Installed value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<serde_json::Value>,
    /// Why the change is notable.
    pub reason: String,
}

/// The complete plan and staging result.
#[derive(Debug, Clone, Serialize)]
pub struct PullResult {
    /// `planned`, `current` or `staged`.
    pub action: String,
    /// Harness target.
    pub target: String,
    /// Skill name.
    pub name: String,
    /// Pending tree, set once staged.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub stage_path: String,
    /// Content and resource changes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<PullChange>,
    /// Frontmatter changes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub frontmatter_changes: Vec<PullFrontmatterChange>,
    /// Reasons a pull was refused.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub refusals: Vec<String>,
}

/// Returns the pending tree for `target`/`name`.
///
/// # Errors
/// Rejects unsafe skill names.
pub fn pending_path(options: &PullOptions) -> Result<PathBuf, SkillError> {
    validate_skill_name(&options.name)?;
    Ok(options
        .pending_dir
        .join(&options.target)
        .join(&options.name))
}

fn never_pulled(target: &str) -> impl Fn(&str) -> bool {
    let metadata = crate::target::lookup(target).and_then(|spec| spec.metadata_file);
    move |path: &str| {
        path == MARKER_FILE
            || path == MANIFEST_FILE
            || path == "agents/openai.yaml"
            || metadata == Some(path)
    }
}

fn installed_tree(options: &PullOptions) -> Result<PathBuf, SkillError> {
    let path = install::install_path_for(
        &options.target,
        &options.home_dir,
        options.project_dir.as_deref(),
        &options.scope,
        &options.name,
    )?;
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(SkillError(format!(
            "installed skill at {} is a symlink install; pull needs a copy-mode install",
            path.display()
        ))),
        Ok(_) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(SkillError(format!(
            "installed skill not found at {}",
            path.display()
        ))),
        Err(error) => Err(SkillError(format!("stat {}: {error}", path.display()))),
    }
}

/// Plans a pull and, unless `dry_run`, stages it as a pending tree.
///
/// # Errors
/// Returns refusals (conflicts, edited overlay regions), a held lock, a
/// missing installed skill and I/O failures.
pub fn pull(options: &PullOptions) -> Result<PullResult, SkillError> {
    if crate::target::lookup(&options.target).is_none() {
        return Err(SkillError(format!("unknown target {:?}", options.target)));
    }
    let _lock = acquire(options)?;
    let library = crate::gotext::join(&options.library_dir, &options.name);
    let bundle = load_bundle(&library)?;
    let installed = installed_tree(options)?;
    let mut result = PullResult {
        action: "planned".into(),
        target: options.target.clone(),
        name: options.name.clone(),
        stage_path: String::new(),
        changes: Vec::new(),
        frontmatter_changes: Vec::new(),
        refusals: Vec::new(),
    };
    let rendered = render_target(&bundle, &options.target, &RenderMetadata::default())?;
    let scratch = tempfile::tempdir().map_err(|error| SkillError(error.to_string()))?;
    let fresh = materialize(&bundle, &rendered, scratch.path())?;
    let left = install::file_hashes(&fresh.root, false)?;
    let right = install::file_hashes(&installed, false)?;
    let base = base_hashes(options);
    let pullable = pullable_paths(&base, &left, &right, &mut result)?;
    let source = read_skill_md(&library.join("SKILL.md"))?;
    let current = read_skill_md(&installed.join("SKILL.md"))?;
    // SKILL.md is one drift unit: when the library changed it (or nothing
    // diverged), the installed text is stale, not an edit to carry back.
    let edited = base.is_empty() || pullable.contains("SKILL.md");
    let mut front = source.front.clone();
    let mut middle = source.body.clone();
    if edited {
        middle = pull_body(&bundle, &options.target, &current.body)
            .map_err(|refusals| SkillError(format!("pull refused: {}", refusals.join("; "))))?;
        if middle.trim_end_matches('\n') != source.body.trim_end_matches('\n') {
            result.changes.push(PullChange {
                path: "SKILL.md".into(),
                status: "modified".into(),
            });
        }
        pull_frontmatter(
            &bundle,
            &options.target,
            &mut front,
            &current.front,
            &mut result,
        )?;
    }
    let skip = never_pulled(&options.target);
    let hashes = Hashes {
        base: &base,
        left: &left,
        right: &right,
    };
    result
        .changes
        .extend(resource_changes(&hashes, &pullable, &skip));
    result.changes.sort_by(|a, b| a.path.cmp(&b.path));
    if result.changes.is_empty() && result.frontmatter_changes.is_empty() {
        result.action = "current".into();
    }
    if result.action == "current" {
        return Ok(result);
    }
    if options.dry_run {
        result.action = "planned".into();
        return Ok(result);
    }
    let stage = pending_path(options)?;
    let skill_md = stage_skill_md(&source.front_text, &source.body, &front, &middle, &result)?;
    stage_tree(
        &stage,
        &library,
        &installed,
        &skill_md,
        &result.changes,
        (&options.target, &options.name),
        &skip,
    )?;
    result.stage_path = stage.to_string_lossy().into_owned();
    result.action = "staged".into();
    Ok(result)
}

/// Digests of the frozen install-time snapshot; empty when there is none.
fn base_hashes(options: &PullOptions) -> BTreeMap<String, String> {
    install::read_manifest_for(
        &options.home_dir,
        options.base_dir.as_deref(),
        &options.target,
        &options.scope,
        &options.name,
        options.project_dir.as_deref(),
    )
    .ok()
    .flatten()
    .map(|manifest| install::manifest_hashes(&manifest))
    .unwrap_or_default()
}

/// Paths a harness edit may be pulled from (harness-changed or converged).
/// Conflicts refuse the whole pull.
fn pullable_paths(
    base: &BTreeMap<String, String>,
    left: &BTreeMap<String, String>,
    right: &BTreeMap<String, String>,
    result: &mut PullResult,
) -> Result<BTreeSet<String>, SkillError> {
    let mut pullable = BTreeSet::new();
    if base.is_empty() {
        return Ok(pullable);
    }
    for drift in install::classify_drift(base, left, right) {
        match drift.kind {
            DriftKind::Conflict => result.refusals.push(format!("conflict in {}", drift.path)),
            DriftKind::HarnessChanged | DriftKind::Converged => {
                pullable.insert(drift.path);
            }
            _ => {}
        }
    }
    if result.refusals.is_empty() {
        return Ok(pullable);
    }
    result.refusals.sort();
    Err(SkillError(format!(
        "pull refused: {}",
        result.refusals.join("; ")
    )))
}

/// File digests of the three sides of a pull.
struct Hashes<'a> {
    base: &'a BTreeMap<String, String>,
    left: &'a BTreeMap<String, String>,
    right: &'a BTreeMap<String, String>,
}

/// Resource-level changes: everything except `SKILL.md` and bookkeeping files.
fn resource_changes(
    hashes: &Hashes<'_>,
    pullable: &BTreeSet<String>,
    skip: &dyn Fn(&str) -> bool,
) -> Vec<PullChange> {
    let (base, left, right) = (hashes.base, hashes.left, hashes.right);
    let change = |path: &String, status: &str| PullChange {
        path: path.clone(),
        status: status.to_owned(),
    };
    let paths: BTreeSet<&String> = left.keys().chain(right.keys()).chain(base.keys()).collect();
    let mut changes = Vec::new();
    for path in paths {
        if skip(path) || path == "SKILL.md" || (!base.is_empty() && !pullable.contains(path)) {
            continue;
        }
        match (left.get(path), right.get(path)) {
            (Some(_), None) if base.contains_key(path) => changes.push(change(path, "removed")),
            (None, Some(_)) => changes.push(change(path, "added")),
            (Some(lhs), Some(rhs)) if lhs != rhs => changes.push(change(path, "modified")),
            _ => {}
        }
    }
    changes
}

/// Re-serializes only what changed so untouched frontmatter stays byte-exact.
fn stage_skill_md(
    front_text: &str,
    body: &str,
    front: &serde_yaml_ng::Mapping,
    middle: &str,
    result: &PullResult,
) -> Result<Vec<u8>, SkillError> {
    let front_out = if result.frontmatter_changes.is_empty() {
        format!("{front_text}\n")
    } else {
        serde_yaml_ng::to_string(front)
            .map_err(|error| SkillError(format!("encode frontmatter: {error}")))?
    };
    let body_changed = middle.trim_end_matches('\n') != body.trim_end_matches('\n');
    let body_out = if body_changed { middle } else { body };
    Ok(format!("---\n{front_out}---\n\n{body_out}").into_bytes())
}

/// Promotes a staged pull into the library. Never touches a harness target.
///
/// # Errors
/// Returns a missing pending pull, validation failures of the staged tree and
/// I/O failures. The library is unchanged when validation fails.
pub fn apply_pending(options: &PullOptions) -> Result<(), SkillError> {
    let _lock = acquire(options)?;
    let stage = pending_path(options)?;
    if !stage.join(MANIFEST_FILE).is_file() {
        return Err(SkillError(format!(
            "no pending pull for {}/{}",
            options.target, options.name
        )));
    }
    let target = crate::gotext::join(&options.library_dir, &options.name);
    if !target.is_dir() {
        return Err(SkillError(format!(
            "library skill {} not found at {}",
            options.name,
            target.display()
        )));
    }
    let work = tempfile::Builder::new()
        .prefix(".pull-tmp-")
        .tempdir_in(&options.library_dir)
        .map_err(|error| SkillError(format!("create pull staging: {error}")))?;
    let fresh = work.path().join(&options.name);
    copy_tree(&stage, &fresh, &[MANIFEST_FILE])?;
    // The directory name matches the skill, so name rules apply as in the library.
    let bundle = load_bundle(&fresh)?;
    let errors: Vec<String> = validate(&bundle)
        .into_iter()
        .filter(|issue| issue.severity == "error")
        .map(|issue| issue.message)
        .collect();
    if !errors.is_empty() {
        return Err(SkillError(format!(
            "pending pull fails validation: {}",
            errors.join("; ")
        )));
    }
    drop(bundle);
    replace_contents(&fresh, &target)?;
    if vcs::is_repo(&target) {
        let message = format!("pull: apply pending changes for {}", options.name);
        if let Err(error) = vcs::commit(&target, &message)
            && !vcs::is_unavailable(&error)
        {
            return Err(error);
        }
    }
    fs::remove_dir_all(&stage).map_err(|error| SkillError(format!("remove pending tree: {error}")))
}

/// Whether `path` names an existing pending pull (for CLI diagnostics).
#[must_use]
pub fn has_pending(options: &PullOptions) -> bool {
    pending_path(options)
        .is_ok_and(|stage: PathBuf| Path::new(&stage).join(MANIFEST_FILE).is_file())
}
