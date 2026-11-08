//! `symbrain skills restore <skill> --to <rev>`.

// Short report strings: building them with `format!` pieces is clearer than `write!`.
#![allow(clippy::format_push_string)]

use std::ffi::OsString;
use std::io::Write;

use serde::Serialize;
use symbrain_core::output::OutputFormat;
use symbrain_skills::install::{self, ConflictPolicy, StatusKind, SyncOptions, SyncResult};
use symbrain_skills::restore::{RestoreOptions, RestoreOutcome, restore_skill};

use super::env::Env;
use super::opargs::{self, Spec};
use super::{cleaned, finish, json_line, ops};

const SPEC: Spec = Spec {
    verb: "restore",
    bools: &["dry-run", "allow-dirty", "sync"],
    values: &["to", "scope"],
    shorts: &[],
    usage: "Usage: symbrain skills restore <skill> --to <rev> [--dry-run] [--allow-dirty] [--sync] [--scope user|project] [--json]\n\n\
Roll a library skill back to a past revision of its per-skill git repository.\n\
The undo is a forward commit, never a reset. The restored state is validated\n\
before anything is written; uncommitted changes are never discarded (use\n\
--allow-dirty to snapshot them first). --sync re-installs stale targets.\n",
};

#[derive(Serialize)]
struct Report<'a> {
    #[serde(flatten)]
    outcome: &'a RestoreOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    synced: Option<&'a [SyncResult]>,
}

pub(super) fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let parsed = match opargs::parse(&SPEC, args, stdout, stderr) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let Some(name) = parsed.positional.first() else {
        return ops::usage(stderr, "restore", "skill name is required");
    };
    let Some(rev) = parsed.value("to").filter(|rev| !rev.is_empty()) else {
        return ops::usage(stderr, "restore", "--to is required");
    };
    let scope = match ops::scope_of(parsed.value("scope")) {
        Ok(scope) => scope,
        Err(message) => return ops::usage(stderr, "restore", message),
    };
    let env = Env::load();
    let outcome = match restore_skill(&RestoreOptions {
        library_dir: cleaned(&env.cfg.library_dir),
        name: name.clone(),
        rev: rev.to_owned(),
        dry_run: parsed.flag("dry-run"),
        allow_dirty: parsed.flag("allow-dirty"),
    }) {
        Ok(outcome) => outcome,
        Err(error) => return ops::fail(stderr, "restore", error),
    };
    let mut synced: Option<Vec<SyncResult>> = None;
    let mut stale_targets: Vec<String> = Vec::new();
    if outcome.action == "restored" {
        match stale_installs(&env, name, &scope) {
            Ok(stale) => stale_targets = stale,
            Err(message) => return ops::fail(stderr, "restore", message),
        }
        if parsed.flag("sync") {
            match resync(&env, name, &scope) {
                Ok(results) => synced = Some(results),
                Err(message) => return ops::fail(stderr, "restore", message),
            }
        }
    }
    let output = match format {
        OutputFormat::Json => json_line(&Report {
            outcome: &outcome,
            synced: synced.as_deref(),
        }),
        OutputFormat::Table => table(&outcome, &stale_targets, synced.as_deref()),
    };
    finish("restore", &output, stdout, stderr)
}

fn table(outcome: &RestoreOutcome, stale: &[String], synced: Option<&[SyncResult]>) -> Vec<u8> {
    let mut text = String::new();
    if outcome.dry_run {
        text.push_str(&format!(
            "would restore {} to {} (validation: ok)\n",
            outcome.name, outcome.rev
        ));
        if outcome.snapshot {
            text.push_str("  would snapshot uncommitted changes first\n");
        }
        for file in &outcome.changed_files {
            text.push_str(&format!("  would change {file}\n"));
        }
    } else {
        let short: String = outcome.head.chars().take(8).collect();
        text.push_str(&format!(
            "restored {} to {} (commit {short})\n",
            outcome.name, outcome.rev
        ));
        match synced {
            Some(results) => {
                for result in results {
                    text.push_str(&format!(
                        "  synced {} {}: {}\n",
                        result.target, result.name, result.action
                    ));
                }
            }
            None if !stale.is_empty() => {
                text.push_str(&format!(
                    "  stale installs: {}; run 'symbrain skills sync' or restore with --sync\n",
                    stale.join(", ")
                ));
            }
            None => {}
        }
    }
    text.into_bytes()
}

fn stale_installs(env: &Env, name: &str, scope: &str) -> Result<Vec<String>, String> {
    if env.home_dir.as_os_str().is_empty() {
        return Ok(Vec::new());
    }
    let rows = install::status(&install::StatusOptions {
        home_dir: env.home_dir.clone(),
        project_dir: Some(env.project_dir.clone()),
        scope: scope.to_owned(),
        library_dir: cleaned(&env.cfg.library_dir),
        base_dir: Some(cleaned(&env.cfg.base_dir)),
        ..install::StatusOptions::default()
    })
    .map_err(|error| format!("post-restore status: {error}"))?;
    Ok(rows
        .into_iter()
        .filter(|row| {
            row.name == name && matches!(row.status, StatusKind::Stale | StatusKind::HarnessChanged)
        })
        .map(|row| row.target)
        .collect())
}

fn resync(env: &Env, name: &str, scope: &str) -> Result<Vec<SyncResult>, String> {
    let rows = install::status(&install::StatusOptions {
        home_dir: env.home_dir.clone(),
        project_dir: Some(env.project_dir.clone()),
        scope: scope.to_owned(),
        library_dir: cleaned(&env.cfg.library_dir),
        base_dir: Some(cleaned(&env.cfg.base_dir)),
        ..install::StatusOptions::default()
    })
    .map_err(|error| format!("post-restore status: {error}"))?;
    let stale: Vec<_> = rows
        .into_iter()
        .filter(|row| {
            row.name == name && matches!(row.status, StatusKind::Stale | StatusKind::HarnessChanged)
        })
        .collect();
    install::sync_selected(
        &SyncOptions {
            library_dir: cleaned(&env.cfg.library_dir),
            home_dir: env.home_dir.clone(),
            project_dir: Some(env.project_dir.clone()),
            scope: scope.to_owned(),
            base_dir: Some(cleaned(&env.cfg.base_dir)),
            render_dir: Some(cleaned(&env.cfg.render_dir)),
            conflict_policy: ConflictPolicy::Abort,
            ..SyncOptions::default()
        },
        &stale,
    )
    .map_err(|error| format!("post-restore sync: {error}"))
}
