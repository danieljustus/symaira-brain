//! Import of skill directories into the managed library (`symbrain skills
//! import`), with per-skill git versioning.
//!
//! A fresh import ends with exactly one commit; `update` replaces the tracked
//! tree in place (the `.git` repository survives) and records a new commit.
//! Versioning failures after a successful copy degrade to a warning.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::model::SkillError;
use crate::tree_copy::{copy_tree, replace_contents};
use crate::{load_bundle, validate, validate_skill_name, vcs};

/// Controls one import.
#[derive(Debug, Clone, Copy)]
pub struct ImportOptions {
    /// Version the skill in its own git repository.
    pub vcs_enabled: bool,
    /// Re-import over an existing library skill instead of refusing.
    pub update: bool,
}

/// Outcome of a single skill import.
#[derive(Debug, Clone)]
pub struct ImportResult {
    /// Skill name.
    pub name: String,
    /// Library directory of the skill.
    pub path: PathBuf,
    /// Whether an existing skill was updated.
    pub updated: bool,
    /// Non-fatal versioning problem.
    pub vcs_warning: String,
}

/// Per-item outcome of a batch import.
#[derive(Debug, Clone, Serialize)]
pub struct BatchResult {
    /// Skill name or source directory name.
    pub name: String,
    /// Library path, set for imported skills.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
    /// `imported` or `failed`.
    pub status: String,
    /// Failure text.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
    /// Non-fatal versioning problem.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub vcs_warning: String,
}

/// Copies one skill directory into `library_dir`.
///
/// The source must load and carry no validation errors, so the library never
/// holds a skill that cannot be rendered.
///
/// # Errors
/// Returns load/validation failures, an existing library entry without
/// `update`, and copy failures.
pub fn import_skill(
    src: &Path,
    library_dir: &Path,
    options: ImportOptions,
) -> Result<ImportResult, SkillError> {
    let bundle = load_bundle(src)?;
    let name = bundle.frontmatter.name.clone();
    if name.is_empty() {
        return Err(SkillError("cannot import skill without name".into()));
    }
    validate_skill_name(&name)?;
    let errors: Vec<String> = validate(&bundle)
        .into_iter()
        .filter(|issue| issue.severity == "error" && issue.code != "name_dir_mismatch")
        .map(|issue| issue.message)
        .collect();
    if !errors.is_empty() {
        return Err(SkillError(format!(
            "skill {} fails validation: {}",
            crate::gotext::quote(&name),
            errors.join("; ")
        )));
    }
    let dst = crate::gotext::join(library_dir, &name);
    let exists = dst.exists();
    if exists && !options.update {
        return Err(SkillError(format!(
            "skill {} already exists in library",
            crate::gotext::quote(&name)
        )));
    }
    fs::create_dir_all(library_dir)
        .map_err(|error| SkillError(format!("create {}: {error}", library_dir.display())))?;
    let operation = if exists {
        let staging = tempfile::Builder::new()
            .prefix(".import-tmp-")
            .tempdir_in(library_dir)
            .map_err(|error| SkillError(format!("create import staging: {error}")))?;
        let fresh = staging.path().join("tree");
        copy_tree(src, &fresh, &[])?;
        replace_contents(&fresh, &dst)?;
        "update"
    } else {
        copy_tree(src, &dst, &[])?;
        "import"
    };
    let mut result = ImportResult {
        name: name.clone(),
        path: dst.clone(),
        updated: exists,
        vcs_warning: String::new(),
    };
    if options.vcs_enabled {
        result.vcs_warning = version(&dst, &name, operation, src);
    }
    Ok(result)
}

/// Best effort: returns a warning instead of failing the import.
fn version(dst: &Path, name: &str, operation: &str, src: &Path) -> String {
    match vcs::init(dst) {
        Err(error) if vcs::is_unavailable(&error) => return String::new(),
        Err(error) => return format!("versioning unavailable for {name}: {error}"),
        Ok(_) => {}
    }
    let message = format!("{operation}: skill {name} from {}", src.display());
    match vcs::commit(dst, &message) {
        Err(error) if !vcs::is_unavailable(&error) => {
            format!("versioning degraded for {name}: {error}")
        }
        _ => String::new(),
    }
}

fn is_skill_dir(path: &Path) -> bool {
    fs::metadata(path.join("SKILL.md")).is_ok_and(|meta| meta.is_file())
}

/// Imports `parent` itself when it is a skill, otherwise each immediate
/// subdirectory holding a `SKILL.md`. One bad item never aborts the rest.
#[must_use]
pub fn import_skills(
    parent: &Path,
    library_dir: &Path,
    options: ImportOptions,
) -> Vec<BatchResult> {
    let base = |path: &Path| {
        path.file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    };
    let outcome = |name: String, result: Result<ImportResult, SkillError>| match result {
        Ok(done) => BatchResult {
            name: done.name,
            path: done.path.to_string_lossy().into_owned(),
            status: "imported".into(),
            error: String::new(),
            vcs_warning: done.vcs_warning,
        },
        Err(error) => BatchResult {
            name,
            path: String::new(),
            status: "failed".into(),
            error: error.0,
            vcs_warning: String::new(),
        },
    };
    if is_skill_dir(parent) {
        return vec![outcome(
            base(parent),
            import_skill(parent, library_dir, options),
        )];
    }
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) => return vec![outcome(base(parent), Err(SkillError(error.to_string())))],
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .filter(|path| is_skill_dir(path))
        .collect();
    dirs.sort();
    dirs.iter()
        .map(|dir| outcome(base(dir), import_skill(dir, library_dir, options)))
        .collect()
}
