//! Helpers shared by the library-operation verbs.

use std::io::Write;
use std::path::{Path, PathBuf};

use symbrain_core::exit;
use symbrain_skills::RenderMetadata;
use symbrain_skills::context_profile;
use symbrain_skills::install::{self, OperationEvent};

use super::env::Env;

/// Prints `symbrain skills <verb>: <message>` and returns the runtime code.
pub(super) fn fail(stderr: &mut dyn Write, verb: &str, message: impl std::fmt::Display) -> u8 {
    let _ = writeln!(stderr, "symbrain skills {verb}: {message}");
    exit::GENERIC
}

/// Prints a usage-class diagnostic.
pub(super) fn usage(stderr: &mut dyn Write, verb: &str, message: impl std::fmt::Display) -> u8 {
    let _ = writeln!(stderr, "symbrain skills {verb}: {message}");
    exit::USAGE
}

/// `user` (default) or `project`.
pub(super) fn scope_of(value: Option<&str>) -> Result<String, String> {
    match value.map(str::trim) {
        None | Some("" | "user") => Ok("user".into()),
        Some("project") => Ok("project".into()),
        Some(other) => Err(format!("unknown scope {other:?} (known: user, project)")),
    }
}

/// Expands `--target` (`all`, one name or a comma list) into registered,
/// enabled target names.
pub(super) fn targets_of(value: Option<&str>, env: &Env) -> Result<Vec<String>, String> {
    let known = symbrain_skills::default_targets();
    let raw = value.map_or("all", str::trim);
    if raw.is_empty() || raw == "all" {
        return Ok(symbrain_skills::enabled_target_names(&env.disabled_targets));
    }
    let mut targets: Vec<String> = Vec::new();
    for part in raw
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        if !known.iter().any(|name| name == part) {
            return Err(format!(
                "unknown target {part:?} (known: {})",
                known.join(", ")
            ));
        }
        if !targets.iter().any(|name| name == part) {
            targets.push(part.to_owned());
        }
    }
    Ok(targets)
}

/// Resolves a skill argument: an existing local path wins, otherwise the name
/// of a library skill.
pub(super) fn resolve_skill_dir(arg: &str, env: &Env) -> Result<PathBuf, String> {
    let local = Path::new(arg);
    let local = if local.is_absolute() {
        local.to_path_buf()
    } else {
        env.project_dir.join(local)
    };
    if local.exists() {
        return Ok(local);
    }
    let library = symbrain_skills::gotext::join(&env.cfg.library_dir, arg);
    if !arg.contains(['/', '\\']) && library.exists() {
        return Ok(library);
    }
    Err(format!(
        "skill {arg:?} not found locally at {:?} nor in library at {:?}",
        local.display().to_string(),
        library.display().to_string()
    ))
}

/// Appends one best-effort record to the shared operation log.
pub(super) fn record(env: &Env, event: &str, skill: &str, target: &str, path: &Path, error: &str) {
    install::record_event(
        Some(&env.log_path),
        OperationEvent {
            event: event.to_owned(),
            skill: skill.to_owned(),
            target: target.to_owned(),
            path: path.to_string_lossy().into_owned(),
            outcome: if error.is_empty() { "ok" } else { "error" }.to_owned(),
            error: error.to_owned(),
            tool_version: env!("CARGO_PKG_VERSION").to_owned(),
            actor: "cli".to_owned(),
            ..OperationEvent::default()
        },
    );
}

/// One skill to install: its directory and the render metadata.
pub(super) struct Job {
    pub path: PathBuf,
    pub metadata: RenderMetadata,
}

pub(super) fn jobs(
    parsed: &super::opargs::Args,
    env: &Env,
    ignore: bool,
) -> Result<Vec<Job>, String> {
    let base = RenderMetadata {
        ignore_capabilities: ignore,
        disabled_targets: env.disabled_targets.clone(),
        ..RenderMetadata::default()
    };
    if let Some(profile) = parsed.value("profile") {
        let project = Some(env.project_dir.as_path());
        let (rows, issues) = context_profile::resolve(
            &super::cleaned(&env.cfg.library_dir),
            &super::cleaned(&env.cfg.profiles_dir),
            project,
            profile,
        )
        .map_err(|error| format!("resolve profile: {error}"))?;
        if let Some(issue) = issues.iter().find(|issue| issue.severity == "error") {
            return Err(format!("resolve profile: {}", issue.message));
        }
        if rows.is_empty() {
            return Err(format!("profile {profile:?} not found or has no links"));
        }
        return Ok(rows
            .into_iter()
            .map(|row| Job {
                path: symbrain_skills::gotext::join(&env.cfg.library_dir, &row.skill),
                metadata: RenderMetadata {
                    alias: row.alias,
                    profile: row.profile,
                    source: row.source,
                    ..base.clone()
                },
            })
            .collect());
    }
    let skill = parsed
        .positional
        .first()
        .ok_or_else(|| "skill-dir or --profile is required".to_owned())?;
    Ok(vec![Job {
        path: resolve_skill_dir(skill, env)?,
        metadata: base,
    }])
}
