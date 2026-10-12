//! Per-skill git operations; configuration and commit identity remain local.
use crate::SkillError;
use serde::Serialize;
use std::path::Path;

#[path = "vcs_restore.rs"]
mod restore;
pub use restore::{extract, restore};

/// One commit in newest-first history order.
#[derive(Debug, Serialize)]
pub struct Commit {
    /// Full commit identity.
    pub revision: String,
    /// Author timestamp from git's %aI.
    pub timestamp: String,
    /// First message line.
    pub subject: String,
    /// import/update/restore or unknown.
    pub operation: String,
    /// Paths changed by this commit.
    pub files: Vec<String>,
}

#[path = "vcs_process.rs"]
mod process;
pub(super) fn run(dir: &Path, args: &[&str]) -> Result<Vec<u8>, SkillError> {
    process::run(dir, args)
}
/// Whether a working repository exists, without initializing it.
#[must_use]
pub fn is_repo(dir: &Path) -> bool {
    dir.join(".git").exists() && run(dir, &["rev-parse", "--git-dir"]).is_ok()
}
/// Resolves a revision to a commit before passing it to extraction.
/// # Errors
/// Returns git discovery/lookup failures.
pub fn resolve(dir: &Path, rev: &str) -> Result<String, SkillError> {
    Ok(String::from_utf8_lossy(&run(
        dir,
        &["rev-parse", "--verify", &format!("{rev}^{{commit}}")],
    )?)
    .trim()
    .into())
}
/// Working-tree dirtiness includes staged and untracked files.
/// # Errors
/// Returns git failures.
pub fn dirty(dir: &Path) -> Result<bool, SkillError> {
    Ok(
        !String::from_utf8_lossy(&run(dir, &["status", "--porcelain"])?)
            .trim()
            .is_empty(),
    )
}
/// Paths differing from the target revision, retaining git's output order.
/// # Errors
/// Returns git failures.
pub fn changed(dir: &Path, rev: &str) -> Result<Vec<String>, SkillError> {
    Ok(
        String::from_utf8_lossy(&run(dir, &["diff", "--name-only", "--no-color", rev])?)
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.trim_end_matches('\r').into())
            .collect(),
    )
}
/// Creates one forward commit with symskills' pinned identity and signing off.
/// # Errors
/// Returns staging/commit failures; a clean tree returns an empty revision.
pub fn commit(dir: &Path, message: &str) -> Result<String, SkillError> {
    if !dirty(dir)? {
        return Ok(String::new());
    }
    run(dir, &["add", "-A"])?;
    run(
        dir,
        &[
            "-c",
            "user.name=symskills",
            "-c",
            "user.email=symskills@localhost",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            message,
        ],
    )?;
    Ok(String::from_utf8_lossy(&run(dir, &["rev-parse", "HEAD"])?)
        .trim()
        .into())
}
/// Reads real git history; no metadata is inferred from filesystem times.
/// # Errors
/// Returns git failures other than an empty repository.
pub fn history(dir: &Path, limit: i64) -> Result<Vec<Commit>, SkillError> {
    let output = match run(
        dir,
        &[
            "log",
            "--no-color",
            &format!("-n{}", if limit <= 0 { 20 } else { limit }),
            "--name-only",
            "--format=%x1e%H%x1f%aI%x1f%s",
        ],
    ) {
        Ok(output) => output,
        Err(error) if error.0.contains("does not have any commits yet") => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let text = String::from_utf8_lossy(&output);
    let mut commits = Vec::new();
    for chunk in text.split('\u{1e}') {
        let mut lines = chunk.trim_matches(['\n', '\r']).lines();
        let Some(first) = lines.next() else { continue };
        let fields: Vec<_> = first.split('\u{1f}').collect();
        if fields.len() < 3 {
            continue;
        }
        let op = fields[2].split_once(':').map_or("unknown", |(op, _)| op);
        commits.push(Commit {
            revision: fields[0].into(),
            timestamp: fields[1].into(),
            subject: fields[2].into(),
            operation: if matches!(op, "import" | "update" | "restore") {
                op
            } else {
                "unknown"
            }
            .into(),
            files: lines
                .filter(|line| !line.is_empty())
                .map(|line| line.trim_end_matches('\r').into())
                .collect(),
        });
    }
    Ok(commits)
}

/// Retained cross-process ownership of one versioned skill and its library.
pub struct RepositoryGuard {
    _locks: crate::install::lock::InstallLocks,
}
/// Serializes a mutation with the existing skills lock implementation.
/// # Errors
/// Returns unsafe-lock or contention errors; read-only callers must not call it.
pub fn lock_repository(dir: &Path) -> Result<RepositoryGuard, SkillError> {
    let mut paths = vec![dir.to_path_buf()];
    if let Some(parent) = dir.parent() {
        paths.push(parent.to_path_buf());
    }
    Ok(RepositoryGuard {
        _locks: crate::install::lock::acquire(&paths)?,
    })
}
