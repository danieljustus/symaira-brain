//! `symbrain skills status` (Go `cmdSkillsStatus`).

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::PathBuf;

use serde::Serialize;
use symbrain_core::config::format_go_quoted_bytes as quote;
use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_skills::install::{self, InstallStatus, RenderStatus, StatusKind, StatusOptions};

use super::env::Env;
use super::flags::{self, Flags, Verb, bytes};
use super::warn::{self, Warned};
use super::{cleaned, finish, json_line};

#[derive(Debug, Default, Serialize)]
struct Summary {
    in_sync: usize,
    stale: usize,
    harness_changed: usize,
    conflict: usize,
    orphaned: usize,
    unmanaged: usize,
}

#[derive(Debug, Serialize)]
struct Report<'a> {
    installs: Vec<Warned<&'a InstallStatus>>,
    summary: Summary,
}

/// Go `resolveTargets` then `resolveScope`: the trimmed target must be empty
/// or a registered target, the trimmed scope empty, `user` or `project`.
/// Diagnostics quote the exact bytes Go quotes.
pub(super) fn resolve_target_scope(
    verb: Verb,
    parsed: &Flags,
    stderr: &mut dyn Write,
) -> Result<(Option<String>, String), u8> {
    let known = symbrain_skills::default_targets();
    let target = bytes::trim_target(&parsed.target);
    let target_name = std::str::from_utf8(target)
        .ok()
        .filter(|name| name.is_empty() || known.iter().any(|known| known == name));
    let Some(target_name) = target_name else {
        let _ = writeln!(
            stderr,
            "symbrain skills {}: unknown target {} (known: {})",
            verb.name(),
            quote(target),
            known.join(", ")
        );
        return Err(exit::USAGE);
    };
    let scope = match bytes::trim_target(&parsed.scope) {
        b"" | b"user" => "user",
        b"project" => "project",
        _ => {
            let _ = writeln!(
                stderr,
                "symbrain skills {}: unknown scope {} (known: user, project)",
                verb.name(),
                quote(&parsed.scope)
            );
            return Err(exit::USAGE);
        }
    };
    Ok((
        (!target_name.is_empty()).then(|| target_name.to_owned()),
        scope.to_owned(),
    ))
}

/// The user-scope render-drift extension (#621) is on for an explicit target
/// when neither `SYMSKILLS_*` variables nor config files customise the run;
/// every other run reports Go's bytes exactly.
pub(super) fn render_extension(env: &Env, target: Option<&str>) -> Option<PathBuf> {
    let target = target?;
    if env.customised {
        return None;
    }
    let rendered = super::skills_data_root().join("rendered");
    (target == "opencode" || rendered.join(target).is_dir()).then_some(rendered)
}

/// Go's `os.UserHomeDir` error text for the scan.
pub(super) const HOME_UNDEFINED: &str = "$HOME is not defined";

pub(super) fn status_options(env: &Env, scope: String, target: Option<String>) -> StatusOptions {
    StatusOptions {
        home_dir: env.home_dir.clone(),
        project_dir: Some(env.project_dir.clone()),
        render_dir: render_extension(env, target.as_deref()),
        targets: env.target_selection(target),
        scope,
        library_dir: cleaned(&env.cfg.library_dir),
        base_dir: Some(cleaned(&env.cfg.base_dir)),
        skills: Vec::new(),
        // Go hands `install.Status` the config cache directory here (#955).
        cached_render_hash: true,
    }
}

pub(super) fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let parsed = match flags::parse(Verb::Status, args, stderr) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let (target, scope) = match resolve_target_scope(Verb::Status, &parsed, stderr) {
        Ok(values) => values,
        Err(code) => return code,
    };
    let env = Env::load();
    if env.home_dir.as_os_str().is_empty() {
        let _ = writeln!(
            stderr,
            "symbrain skills status: scan installs: {HOME_UNDEFINED}"
        );
        return exit::GENERIC;
    }
    let installs = match install::status(&status_options(&env, scope, target)) {
        Ok(rows) => rows,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain skills status: scan installs: {error}");
            return exit::GENERIC;
        }
    };

    let mut summary = Summary::default();
    for row in &installs {
        match row.status {
            StatusKind::InSync | StatusKind::Converged => summary.in_sync += 1,
            StatusKind::Stale => summary.stale += 1,
            StatusKind::HarnessChanged => summary.harness_changed += 1,
            StatusKind::Conflict => summary.conflict += 1,
            StatusKind::Orphaned => summary.orphaned += 1,
            StatusKind::Unmanaged => summary.unmanaged += 1,
        }
    }
    let library = cleaned(&env.cfg.library_dir);
    let installs_tagged = warn::tag_all(&installs, |row| {
        symbrain_skills::gotext::join(&library, &row.name)
    });
    let output = match format {
        OutputFormat::Json => json_line(&Report {
            installs: installs_tagged,
            summary,
        }),
        OutputFormat::Table => {
            warn::print("status", &installs_tagged, stderr);
            table(&installs)
        }
    };
    finish("status", &output, stdout, stderr)
}

fn status_name(status: StatusKind) -> &'static str {
    match status {
        StatusKind::InSync => "in-sync",
        StatusKind::Stale => "stale",
        StatusKind::HarnessChanged => "harness-changed",
        StatusKind::Conflict => "conflict",
        StatusKind::Converged => "converged",
        StatusKind::Orphaned => "orphaned",
        StatusKind::Unmanaged => "unmanaged",
    }
}

fn table(installs: &[InstallStatus]) -> Vec<u8> {
    if installs.is_empty() {
        return b"No installed skills found.\n".to_vec();
    }
    let has_render = installs.iter().any(|row| row.render_status.is_some());
    let mut out = Vec::new();
    out.extend_from_slice(b"TARGET\tSKILL\tSTATUS\tMODE\tPATH");
    if has_render {
        out.extend_from_slice(b"\tRENDER");
    }
    out.push(b'\n');
    for row in installs {
        let mode = row.mode.as_deref().filter(|mode| !mode.trim().is_empty());
        out.extend_from_slice(
            format!(
                "{}\t{}\t{}\t{}\t",
                row.target,
                row.name,
                status_name(row.status),
                mode.unwrap_or("-")
            )
            .as_bytes(),
        );
        out.extend_from_slice(OsStr::as_encoded_bytes(row.path.as_os_str()));
        if has_render {
            out.extend_from_slice(
                format!("\t{}", row.render_status.map_or("-", RenderStatus::label)).as_bytes(),
            );
        }
        out.push(b'\n');
    }
    out
}
