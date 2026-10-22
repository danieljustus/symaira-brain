//! Per-skill git operations behind `skills_history` / `skills_restore`.
//!
//! Every repository access goes through the `git` binary found on `PATH`, with
//! the same pinned environment, commit identity and force-disabled signing as
//! the Go implementation. A missing binary degrades to "not a repository".

use crate::SkillError;
use serde::Serialize;
use std::path::Path;
use std::process::{Command, Stdio};

#[path = "vcs_restore.rs"]
mod restore;
pub use restore::{extract, restore};

/// One commit in newest-first history order.
#[derive(Debug, Serialize)]
pub struct Commit {
    /// Full commit identity.
    pub revision: String,
    /// Author timestamp from git's `%aI`.
    pub timestamp: String,
    /// First message line.
    pub subject: String,
    /// `import`/`update`/`restore` or `unknown`.
    pub operation: String,
    /// Paths changed by this commit.
    pub files: Vec<String>,
}

const GIT_UNAVAILABLE: &str = "git binary not available";

fn command(dir: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("LANGUAGE", "")
        .stdin(Stdio::null());
    command
}

fn failure(args: &[&str], code: Option<i32>, output: &[u8]) -> SkillError {
    let status = code.map_or_else(
        || "signal: killed".to_owned(),
        |code| format!("exit status {code}"),
    );
    SkillError(format!(
        "git {}: {status}: {}",
        args.join(" "),
        String::from_utf8_lossy(output).trim()
    ))
}

/// Combined stdout+stderr (one shared file offset, like Go's single buffer).
pub(crate) fn run(dir: &Path, args: &[&str]) -> Result<Vec<u8>, SkillError> {
    let capture = tempfile::tempfile().map_err(|error| SkillError(error.to_string()))?;
    let mut shared = capture
        .try_clone()
        .map_err(|error| SkillError(error.to_string()))?;
    let status = command(dir, args)
        .stdout(
            capture
                .try_clone()
                .map_err(|error| SkillError(error.to_string()))?,
        )
        .stderr(capture)
        .status()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                SkillError(GIT_UNAVAILABLE.into())
            } else {
                SkillError(format!("git {}: {error}", args.join(" ")))
            }
        })?;
    read_all(&mut shared, args, status.code(), status.success())
}

fn read_all(
    file: &mut std::fs::File,
    args: &[&str],
    code: Option<i32>,
    success: bool,
) -> Result<Vec<u8>, SkillError> {
    use std::io::{Read, Seek, SeekFrom};
    file.seek(SeekFrom::Start(0))
        .map_err(|error| SkillError(error.to_string()))?;
    let mut output = Vec::new();
    file.take(crate::MAX_TOTAL_RESOURCE_BYTES + 1)
        .read_to_end(&mut output)
        .map_err(|error| SkillError(error.to_string()))?;
    if output.len() as u64 > crate::MAX_TOTAL_RESOURCE_BYTES {
        return Err(SkillError("git output exceeds skills input limit".into()));
    }
    if success {
        Ok(output)
    } else {
        Err(failure(args, code, &output))
    }
}

/// `git archive` keeps archive bytes (stdout) apart from diagnostics (stderr).
pub(crate) fn archive(dir: &Path, rev: &str) -> Result<Vec<u8>, SkillError> {
    let args = ["archive", "--format=tar", rev];
    let out = tempfile::tempfile().map_err(|error| SkillError(error.to_string()))?;
    let err = tempfile::tempfile().map_err(|error| SkillError(error.to_string()))?;
    let mut out_read = out
        .try_clone()
        .map_err(|error| SkillError(error.to_string()))?;
    let mut err_read = err
        .try_clone()
        .map_err(|error| SkillError(error.to_string()))?;
    let status = command(dir, &args)
        .stdout(out)
        .stderr(err)
        .status()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                SkillError(GIT_UNAVAILABLE.into())
            } else {
                SkillError(format!("git archive {rev}: {error}"))
            }
        })?;
    if status.success() {
        return read_all(&mut out_read, &args, status.code(), true);
    }
    let diagnostics = read_all(&mut err_read, &args, status.code(), true).unwrap_or_default();
    Err(SkillError(format!(
        "git archive {rev}: {}: {}",
        status.code().map_or_else(
            || "signal: killed".to_owned(),
            |code| format!("exit status {code}")
        ),
        String::from_utf8_lossy(&diagnostics).trim()
    )))
}

/// Whether a working repository exists, without initializing it.
#[must_use]
pub fn is_repo(dir: &Path) -> bool {
    dir.join(".git").exists() && run(dir, &["rev-parse", "--git-dir"]).is_ok()
}

/// Resolves a revision to a full commit hash.
///
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
///
/// # Errors
/// Returns git failures.
pub fn dirty(dir: &Path) -> Result<bool, SkillError> {
    Ok(
        !String::from_utf8_lossy(&run(dir, &["status", "--porcelain"])?)
            .trim()
            .is_empty(),
    )
}

/// Paths differing from the target revision, in git's output order.
///
/// # Errors
/// Returns git failures.
pub fn changed(dir: &Path, rev: &str) -> Result<Vec<String>, SkillError> {
    Ok(
        String::from_utf8_lossy(&run(dir, &["diff", "--name-only", "--no-color", rev])?)
            .split('\n')
            .map(|line| line.trim_end_matches('\r'))
            .filter(|line| !line.trim().is_empty())
            .map(str::to_owned)
            .collect(),
    )
}

/// Creates one forward commit with the pinned identity and signing off; a
/// clean tree returns an empty revision.
///
/// # Errors
/// Returns staging/commit failures.
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
///
/// # Errors
/// Returns git failures other than an empty repository.
pub fn history(dir: &Path, limit: i64) -> Result<Vec<Commit>, SkillError> {
    let limit = if limit <= 0 { 20 } else { limit };
    let output = match run(
        dir,
        &[
            "log",
            "--no-color",
            &format!("-n{limit}"),
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
        let chunk = chunk.trim_matches(['\n', '\r']);
        if chunk.is_empty() {
            continue;
        }
        let mut lines = chunk.split('\n');
        let first = lines.next().unwrap_or_default();
        let fields: Vec<_> = first.split('\u{1f}').collect();
        if fields.len() < 3 {
            continue;
        }
        let operation = match fields[2].split_once(':') {
            Some((op @ ("import" | "update" | "restore"), _)) => op,
            _ => "unknown",
        };
        commits.push(Commit {
            revision: fields[0].into(),
            timestamp: fields[1].into(),
            subject: fields[2].into(),
            operation: operation.into(),
            files: lines
                .map(|line| line.trim_end_matches('\r'))
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect(),
        });
    }
    Ok(commits)
}
