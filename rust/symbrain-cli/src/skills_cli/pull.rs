//! `symbrain skills pull <skill> [--apply]`.

// Short report strings: building them with `format!` pieces is clearer than `write!`.
#![allow(clippy::format_push_string)]

use std::ffi::OsString;
use std::io::Write;

use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_skills::pull::{self, PullOptions, PullResult};

use super::env::Env;
use super::opargs::{self, Spec};
use super::{cleaned, finish, json_line, ops};

const SPEC: Spec = Spec {
    verb: "pull",
    bools: &["apply", "dry-run"],
    values: &["target", "scope"],
    shorts: &[],
    usage: "Usage: symbrain skills pull <skill> [--target all|a,b] [--scope user|project] [--dry-run] [--apply] [--json]\n\n\
Carry harness-side edits of an installed skill back into the library. A pull\n\
compares the installed copy with a fresh render of its library source and\n\
stages only the harness-side changes as a pending tree; it never writes the\n\
library. --apply promotes the pending tree into the library (validated, and\n\
committed when the skill is versioned). --dry-run prints the plan. Overlay-\n\
produced body regions and frontmatter keys are refused rather than merged.\n",
};

fn describe(result: &PullResult) -> String {
    let mut text = format!("{} {}/{}\n", result.action, result.target, result.name);
    for change in &result.changes {
        text.push_str(&format!("  {} {}\n", change.status, change.path));
    }
    for change in &result.frontmatter_changes {
        text.push_str(&format!(
            "  frontmatter {} ({})\n",
            change.key, change.reason
        ));
    }
    if !result.stage_path.is_empty() {
        text.push_str(&format!("pending: {}\n", result.stage_path));
    }
    text
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
        return ops::usage(stderr, "pull", "skill name is required");
    };
    let env = Env::load();
    let scope = match ops::scope_of(parsed.value("scope")) {
        Ok(scope) => scope,
        Err(message) => return ops::usage(stderr, "pull", message),
    };
    let targets = match ops::targets_of(parsed.value("target"), &env) {
        Ok(targets) => targets,
        Err(message) => return ops::usage(stderr, "pull", message),
    };
    if env.home_dir.as_os_str().is_empty() {
        return ops::fail(stderr, "pull", "$HOME is not defined");
    }
    let explicit = parsed
        .value("target")
        .is_some_and(|value| value.trim() != "all" && !value.trim().is_empty());
    let library = cleaned(&env.cfg.library_dir);
    let pending = library
        .parent()
        .map_or_else(|| library.join("pending"), |dir| dir.join("pending"));
    let mut results: Vec<PullResult> = Vec::new();
    let mut lines = String::new();
    let mut failed = false;
    let mut attempted = 0;
    for target in &targets {
        let options = PullOptions {
            home_dir: env.home_dir.clone(),
            project_dir: Some(env.project_dir.clone()),
            scope: scope.clone(),
            library_dir: library.clone(),
            pending_dir: pending.clone(),
            base_dir: Some(cleaned(&env.cfg.base_dir)),
            target: target.clone(),
            name: name.clone(),
            dry_run: parsed.flag("dry-run"),
        };
        if parsed.flag("apply") {
            if !explicit && !pull::has_pending(&options) {
                continue;
            }
            attempted += 1;
            match pull::apply_pending(&options) {
                Ok(()) => lines.push_str(&format!("applied {target} {name}\n")),
                Err(error) => {
                    failed = true;
                    let _ = writeln!(stderr, "symbrain skills pull: {target}/{name}: {error}");
                }
            }
            continue;
        }
        match pull::pull(&options) {
            Ok(result) => {
                attempted += 1;
                lines.push_str(&describe(&result));
                results.push(result);
            }
            // With the implicit `all` target, an install that does not exist
            // on a harness is simply not a pull candidate there.
            Err(error) if !explicit && error.0.starts_with("installed skill not found") => {}
            Err(error) => {
                attempted += 1;
                failed = true;
                let _ = writeln!(stderr, "symbrain skills pull: {target}/{name}: {error}");
            }
        }
    }
    if attempted == 0 {
        let what = if parsed.flag("apply") {
            "no pending pull"
        } else {
            "not installed"
        };
        return ops::fail(
            stderr,
            "pull",
            format!("skill {name:?}: {what} on any selected target"),
        );
    }
    let output = match format {
        OutputFormat::Json if parsed.flag("apply") => {
            json_line(&serde_json::json!({ "name": name, "applied": !failed }))
        }
        OutputFormat::Json if explicit && results.len() == 1 => json_line(&results[0]),
        OutputFormat::Json => json_line(&results),
        OutputFormat::Table => lines.into_bytes(),
    };
    let code = finish("pull", &output, stdout, stderr);
    if code == exit::OK && failed {
        exit::GENERIC
    } else {
        code
    }
}
