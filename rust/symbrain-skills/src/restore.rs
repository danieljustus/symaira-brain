//! Roll a library skill back to an earlier revision (`symbrain skills
//! restore`) by forward commit; history is never rewritten.
//!
//! The restored state is validated before anything is written, and uncommitted
//! changes are never discarded: a dirty skill is refused unless the caller
//! allows a pre-restore snapshot commit.

use std::path::PathBuf;

use serde::Serialize;

use crate::model::SkillError;
use crate::{gotext, load_bundle, validate, validate_skill_name, vcs};

/// Inputs of one restore.
#[derive(Debug, Clone)]
pub struct RestoreOptions {
    /// Library directory.
    pub library_dir: PathBuf,
    /// Skill name.
    pub name: String,
    /// Revision (hash, prefix, `HEAD~1`, ...).
    pub rev: String,
    /// Plan only.
    pub dry_run: bool,
    /// Snapshot uncommitted changes instead of refusing.
    pub allow_dirty: bool,
}

/// Plan or result of a restore.
#[derive(Debug, Clone, Serialize)]
pub struct RestoreOutcome {
    /// Skill name.
    pub name: String,
    /// Resolved full revision.
    pub rev: String,
    /// Whether nothing was written.
    pub dry_run: bool,
    /// `planned` or `restored`.
    pub action: String,
    /// Files that differ from the target revision.
    pub changed_files: Vec<String>,
    /// New head commit, set once restored.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub head: String,
    /// Whether uncommitted changes were (or would be) snapshotted.
    #[serde(skip)]
    pub snapshot: bool,
}

fn quoted(name: &str) -> String {
    gotext::quote(name)
}

/// Plans (and unless `dry_run`, performs) the restore.
///
/// # Errors
/// Returns unversioned skills, unknown revisions, dirty trees without
/// `allow_dirty`, invalid restored states and git failures.
pub fn restore_skill(options: &RestoreOptions) -> Result<RestoreOutcome, SkillError> {
    let (name, rev) = (options.name.as_str(), options.rev.as_str());
    if name.is_empty() || rev.is_empty() {
        return Err(SkillError("name and rev are required".into()));
    }
    validate_skill_name(name)?;
    let dir = gotext::join(&options.library_dir, name);
    if !vcs::is_repo(&dir) {
        return Err(SkillError(format!(
            "skill {} is not versioned: no git repository at {}",
            quoted(name),
            dir.display()
        )));
    }
    let resolved = vcs::resolve(&dir, rev)
        .map_err(|error| SkillError(format!("revision {} not found: {error}", quoted(rev))))?;
    let dirty =
        vcs::dirty(&dir).map_err(|error| SkillError(format!("check working tree: {error}")))?;
    if dirty && !options.allow_dirty {
        return Err(SkillError(format!(
            "skill {} has uncommitted changes; they are never discarded — pass --allow-dirty to snapshot them into a pre-restore commit first",
            quoted(name)
        )));
    }
    let scratch = tempfile::Builder::new()
        .prefix("symskills-restore-")
        .tempdir()
        .map_err(|error| SkillError(format!("restore skill: {error}")))?;
    let tree = gotext::join(scratch.path(), name);
    vcs::extract(&dir, &resolved, &tree)
        .map_err(|error| SkillError(format!("extract revision: {error}")))?;
    validate_tree(&resolved, &tree)?;
    let changed = vcs::changed(&dir, &resolved)
        .map_err(|error| SkillError(format!("compare revision: {error}")))?;
    let mut outcome = RestoreOutcome {
        name: name.to_owned(),
        rev: resolved.clone(),
        dry_run: options.dry_run,
        action: "planned".into(),
        changed_files: changed,
        head: String::new(),
        snapshot: dirty,
    };
    if options.dry_run {
        return Ok(outcome);
    }
    if dirty {
        vcs::commit(
            &dir,
            &format!("restore: snapshot uncommitted changes before restore to {resolved}"),
        )
        .map_err(|error| SkillError(format!("snapshot uncommitted changes: {error}")))?;
    }
    outcome.head = vcs::restore(&dir, &tree, &format!("restore: skill {name} to {resolved}"))
        .map_err(|error| SkillError(format!("restore skill: {error}")))?;
    outcome.action = "restored".into();
    Ok(outcome)
}

fn validate_tree(rev: &str, tree: &std::path::Path) -> Result<(), SkillError> {
    let bundle = load_bundle(tree).map_err(|error| {
        SkillError(format!(
            "refusing restore to {rev}: restored state is not a valid skill: {error}"
        ))
    })?;
    let errors: Vec<_> = validate(&bundle)
        .into_iter()
        .filter(|issue| issue.severity == "error")
        .map(|issue| issue.message)
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(SkillError(format!(
            "refusing restore to {rev}: restored state fails validation: {}",
            errors.join("; ")
        )))
    }
}
