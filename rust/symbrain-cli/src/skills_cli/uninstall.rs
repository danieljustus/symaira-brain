//! `symbrain skills uninstall <skill> [--target all|a,b]` (#1015).

// Short report strings: building them with `format!` pieces is clearer than `write!`.
#![allow(clippy::format_push_string)]

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_skills::install::{self, InstallOptions};
use symbrain_skills::validate_skill_name;

use super::env::Env;
use super::opargs::{self, Spec};
use super::{cleaned, finish, json_line, ops, serialize_path};

const SPEC: Spec = Spec {
    verb: "uninstall",
    bools: &["dry-run"],
    values: &["target", "scope", "output"],
    shorts: &[('o', "output")],
    usage: "Usage: symbrain skills uninstall <skill> [flags]\n\n\
Flags:\n\
  --target all|a,b       harness targets (default all enabled)\n\
  --scope user|project   install scope (default user)\n\
  --dry-run              plan without removing\n\
  -o, --output DIR       render output directory\n\n\
Remove the managed install of a skill from every selected target and report one\n\
result per target. A failing target does not stop the others. The generated\n\
render copy <render dir>/<target>/<skill> is removed with the install and\n\
reported as render_dir/render_removed.\n",
};

/// One target's outcome; `removed` and `target` keep symskills' key order.
#[derive(Serialize)]
struct Item {
    removed: bool,
    target: String,
    name: String,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_opt_path"
    )]
    render_dir: Option<PathBuf>,
    render_removed: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    error: String,
}

#[allow(clippy::ref_option)]
fn serialize_opt_path<S: serde::Serializer>(
    path: &Option<PathBuf>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match path {
        Some(path) => serialize_path(path, serializer),
        None => serializer.serialize_none(),
    }
}

/// Removes the generated render copy; `Some(path)` when one existed.
fn remove_render(path: &Path, dry_run: bool) -> (Option<PathBuf>, bool, String) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return (None, false, String::new());
    };
    if dry_run {
        return (Some(path.to_path_buf()), false, String::new());
    }
    let result = if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    match result {
        Ok(()) => (Some(path.to_path_buf()), true, String::new()),
        Err(error) => (
            Some(path.to_path_buf()),
            false,
            format!("remove render dir: {error}"),
        ),
    }
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
    let [name] = parsed.positional.as_slice() else {
        return ops::usage(stderr, "uninstall", "exactly one skill name is required");
    };
    if let Err(error) = validate_skill_name(name) {
        return ops::usage(stderr, "uninstall", error);
    }
    let env = Env::load();
    let scope = match ops::scope_of(parsed.value("scope")) {
        Ok(scope) => scope,
        Err(message) => return ops::usage(stderr, "uninstall", message),
    };
    let targets = match ops::targets_of(parsed.value("target"), &env) {
        Ok(targets) => targets,
        Err(message) => return ops::usage(stderr, "uninstall", message),
    };
    if env.home_dir.as_os_str().is_empty() {
        return ops::fail(
            stderr,
            "uninstall",
            "resolve home directory: $HOME is not defined",
        );
    }
    let render_dir = parsed.value("output").map_or_else(
        || cleaned(&env.cfg.render_dir),
        |dir| env.project_dir.join(dir),
    );
    let dry_run = parsed.flag("dry-run");
    let options = InstallOptions {
        home_dir: env.home_dir.clone(),
        project_dir: (scope == "project").then(|| env.project_dir.clone()),
        base_dir: Some(cleaned(&env.cfg.base_dir)),
        dry_run,
        events_path: Some(env.log_path.clone()),
        ..InstallOptions::default()
    };
    let mut failed = false;
    let items: Vec<Item> = targets
        .iter()
        .map(|target| {
            let (removed, mut error) = match install::uninstall(target, name, &options) {
                Ok(removed) => (removed, String::new()),
                Err(error) => (false, error.to_string()),
            };
            // Only a removed install may drop its render copy: a refused or
            // failed uninstall leaves the install pointing at it.
            let (render, render_removed, render_error) = if removed {
                remove_render(&render_dir.join(target).join(name), dry_run)
            } else {
                (None, false, String::new())
            };
            if error.is_empty() {
                error = render_error;
            }
            failed |= !error.is_empty();
            Item {
                removed,
                target: target.clone(),
                name: name.clone(),
                render_dir: render,
                render_removed,
                error,
            }
        })
        .collect();
    for item in items.iter().filter(|item| !item.error.is_empty()) {
        let _ = writeln!(
            stderr,
            "symbrain skills uninstall: {name} on {}: {}",
            item.target, item.error
        );
    }
    let single = parsed.value("target").is_some_and(|t| t.trim() != "all") && items.len() == 1;
    let output = match format {
        OutputFormat::Json if single => json_line(&items[0]),
        OutputFormat::Json => json_line(&items),
        OutputFormat::Table => table(&items, dry_run),
    };
    let code = finish("uninstall", &output, stdout, stderr);
    if code == exit::OK && failed {
        exit::GENERIC
    } else {
        code
    }
}

fn table(items: &[Item], dry_run: bool) -> Vec<u8> {
    items
        .iter()
        .flat_map(|item| {
            let state = match (item.removed, dry_run) {
                (true, true) => "would remove",
                (true, false) => "removed",
                (false, _) => "not installed",
            };
            let mut line = format!("{state} {} on {}", item.name, item.target);
            if let Some(render) = &item.render_dir {
                let verb = if item.render_removed {
                    "removed"
                } else if dry_run {
                    "would remove"
                } else {
                    "kept"
                };
                line.push_str(&format!(" ({verb} render dir {})", render.display()));
            }
            line.push('\n');
            line.into_bytes()
        })
        .collect()
}
