//! Stable render-cache paths for managed symlink installs.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::InstallOptions;
use super::destination::scope_name;
use crate::model::SkillError;
use crate::render::Rendered;

pub(crate) fn cache_path(
    cache: &Path,
    rendered: &Rendered,
    options: &InstallOptions,
) -> Result<PathBuf, SkillError> {
    let scope = scope_name(options);
    let identity = if scope == "project" {
        let project = options
            .project_dir
            .as_deref()
            .ok_or_else(|| SkillError("project scope requires a project directory".to_owned()))?;
        let absolute = std::path::absolute(project)
            .map_err(|error| SkillError(format!("resolve project cache identity: {error}")))?;
        let digest = Sha256::digest(absolute.to_string_lossy().as_bytes());
        format!("{digest:x}")
    } else {
        "user".to_owned()
    };
    Ok(cache
        .join(&rendered.target)
        .join(scope)
        .join(identity)
        .join(&rendered.name))
}
