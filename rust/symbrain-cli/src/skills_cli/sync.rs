//! `symbrain skills sync` (Go `cmdSkillsSync`).

use std::ffi::OsString;
use std::io::Write;

use serde::Serialize;
use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_skills::install::{self, SyncOptions, SyncResult};

use super::env::Env;
use super::flags::{self, Verb};
use super::status::{HOME_UNDEFINED, resolve_target_scope};
use super::{cleaned, finish, json_line};

#[derive(Debug, Serialize)]
struct Report {
    results: Vec<SyncResult>,
    dry_run: bool,
}

pub(super) fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let parsed = match flags::parse(Verb::Sync, args, stderr) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let (target, scope) = match resolve_target_scope(Verb::Sync, &parsed, stderr) {
        Ok(values) => values,
        Err(code) => return code,
    };
    let env = Env::load();
    if env.home_dir.as_os_str().is_empty() {
        let _ = writeln!(stderr, "symbrain skills sync: scan installs: {HOME_UNDEFINED}");
        return exit::GENERIC;
    }
    let dry_run = parsed.dry_run;
    let options = SyncOptions {
        library_dir: cleaned(&env.cfg.library_dir),
        home_dir: env.home_dir.clone(),
        project_dir: Some(env.project_dir.clone()),
        scope,
        targets: target.into_iter().collect(),
        skills: Vec::new(),
        base_dir: Some(cleaned(&env.cfg.base_dir)),
        render_dir: Some(cleaned(&env.cfg.render_dir)),
        // An empty mode preserves the marker's original copy/symlink mode.
        mode: String::new(),
        force: false,
        dry_run,
        conflict_policy: install::ConflictPolicy::Abort,
        events_path: None,
    };
    let results = match install::sync(&options) {
        Ok(results) => results,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain skills sync: {error}");
            return exit::GENERIC;
        }
    };
    let report = Report { results, dry_run };
    let output = match format {
        OutputFormat::Json => json_line(&report),
        OutputFormat::Table => table(&report.results),
    };
    finish("sync", &output, stdout, stderr)
}

fn table(results: &[SyncResult]) -> Vec<u8> {
    if results.is_empty() {
        return b"Every installed skill is in sync.\n".to_vec();
    }
    let mut text = String::from("TARGET\tSKILL\tACTION\tDETAIL\n");
    for result in results {
        let detail = if result.error.trim().is_empty() {
            "-"
        } else {
            &result.error
        };
        text.push_str(&format!(
            "{}\t{}\t{}\t{detail}\n",
            result.target, result.name, result.action
        ));
    }
    text.into_bytes()
}
