//! `symbrain skills install [skill] [--profile P]`.

use std::ffi::OsString;
use std::io::Write;

use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_skills::install::{self, InstallOptions, InstallResult};
use symbrain_skills::{load_bundle, render_target};

use super::env::Env;
use super::opargs::{self, Spec};
use super::warn::{self, Dirty, Warned};
use super::{cleaned, finish, json_line, ops};

const SPEC: Spec = Spec {
    verb: "install",
    bools: &[
        "dry-run",
        "require-clean",
        "force",
        "allow-executable",
        "ignore-capabilities",
    ],
    values: &["target", "scope", "mode", "output", "profile"],
    shorts: &[('o', "output")],
    usage: "Usage: symbrain skills install [skill-dir|library-skill] [flags]\n\n\
Flags:\n\
  --profile NAME         install every skill of a context profile\n\
  --target all|a,b       harness targets (default all enabled)\n\
  --scope user|project   install scope (default user)\n\
  --mode symlink|copy    install mode (default symlink)\n\
  --dry-run              plan without writing\n\
  --require-clean        refuse a source with uncommitted git changes\n\\
  --force                adopt an unmanaged skill at the target path\n\
  --allow-executable     keep executable bits on resource files\n\
  --ignore-capabilities  install despite unsupported target capabilities\n\
  -o, --output DIR       render output directory\n\n\
Render a skill (or every skill of a context profile) and install it into the\n\
selected harness targets. --force adopts an unmanaged skill at the target path,\n\
moving the existing one to a backup directory. --dry-run plans without writing.\n",
};

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
    let env = Env::load();
    let scope = match ops::scope_of(parsed.value("scope")) {
        Ok(scope) => scope,
        Err(message) => return ops::usage(stderr, "install", message),
    };
    let mode = match parsed.value("mode").map(str::trim) {
        None | Some("" | "symlink") => "symlink",
        Some("copy") => "copy",
        Some(other) => {
            let message = format!("unknown mode {other:?} (known: symlink, copy)");
            return ops::usage(stderr, "install", message);
        }
    };
    let targets = match ops::targets_of(parsed.value("target"), &env) {
        Ok(targets) => targets,
        Err(message) => return ops::usage(stderr, "install", message),
    };
    let jobs = match ops::jobs(&parsed, &env, parsed.flag("ignore-capabilities")) {
        Ok(jobs) => jobs,
        Err(message) => return ops::fail(stderr, "install", message),
    };
    if env.home_dir.as_os_str().is_empty() {
        return ops::fail(
            stderr,
            "install",
            "resolve home directory: $HOME is not defined",
        );
    }
    let render_dir = parsed.value("output").map_or_else(
        || cleaned(&env.cfg.render_dir),
        |dir| env.project_dir.join(dir),
    );
    let mut results = Vec::new();
    let mut dirty = Dirty::default();
    let mut failed = false;
    for job in &jobs {
        if parsed.flag("require-clean") && dirty.check(&job.path) {
            let message = format!(
                "{} has uncommitted changes (source-dirty)",
                job.path.display()
            );
            return ops::fail(stderr, "install", message);
        }
        let bundle = match load_bundle(&job.path) {
            Ok(bundle) => bundle,
            Err(error) => return ops::fail(stderr, "install", format!("load skill: {error}")),
        };
        for target in &targets {
            let options = InstallOptions {
                home_dir: env.home_dir.clone(),
                project_dir: (scope == "project").then(|| env.project_dir.clone()),
                base_dir: Some(cleaned(&env.cfg.base_dir)),
                render_dir: Some(render_dir.clone()),
                mode: mode.to_owned(),
                allow_executable: parsed.flag("allow-executable")
                    || bundle.manifest.skill.allow_executable,
                force: parsed.flag("force"),
                dry_run: parsed.flag("dry-run"),
                fault: None,
                events_path: Some(env.log_path.clone()),
            };
            let outcome = render_target(&bundle, target, &job.metadata)
                .and_then(|rendered| install::install_rendered(&bundle, &rendered, &options));
            match outcome {
                Ok(result) => results.push(dirty.tag(result, job.path.clone())),
                Err(error) => {
                    failed = true;
                    let _ = writeln!(
                        stderr,
                        "symbrain skills install: {} on {target}: {error}",
                        bundle.frontmatter.name
                    );
                }
            }
        }
    }
    let output = match format {
        OutputFormat::Json if jobs.len() == 1 && targets.len() == 1 && results.len() == 1 => {
            json_line(&results[0])
        }
        OutputFormat::Json => json_line(&results),
        OutputFormat::Table => table(&results),
    };
    if format == OutputFormat::Table {
        warn::print("install", &results, stderr);
    }
    let code = finish("install", &output, stdout, stderr);
    if code == exit::OK && failed {
        exit::GENERIC
    } else {
        code
    }
}

fn table(results: &[Warned<InstallResult>]) -> Vec<u8> {
    results
        .iter()
        .flat_map(|warned| {
            let result = &warned.row;
            let verb = if result.action == "planned" {
                "planned"
            } else {
                "installed"
            };
            format!("{verb} {} at {}\n", result.name, result.path.display()).into_bytes()
        })
        .collect()
}
