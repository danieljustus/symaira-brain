//! Native skills dispatcher; every skills form is owned in process.
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use symbrain_core::{exit, output::OutputFormat};
#[path = "skills_cli_flags.rs"]
mod flags;
#[path = "skills_cli_list.rs"]
mod list;
#[path = "skills_cli_misc.rs"]
mod misc;
#[path = "skills_cli_status.rs"]
mod status;
#[path = "skills_sync_flags.rs"]
mod sync_flags;
const SKILLS_USAGE: &str = r"symbrain skills — embedded skill library operations

Usage:
  symbrain skills <subcommand> [flags]

Subcommands:
  list        List the skills in the library with their install state
  status      Classify installed skills against the library (drift report)
  targets     Show the harness targets skills can be installed into
  log         Read the local skill operation log
  sync        Repair drifted installs (use --dry-run to see the plan first)
  doctor      Report the configured skill paths and target roots

Use --output table|json (or --json) for the result format. status, sync and
targets accept --scope user|project; status, sync and log accept --target.
Run 'symbrain skills <subcommand> --help' for details.
";

pub fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> Option<u8> {
    if args.is_empty() {
        let _ = write!(stderr, "{SKILLS_USAGE}");
        return Some(exit::USAGE);
    }

    let verb = args[0].to_string_lossy();
    let rest = &args[1..];

    Some(match verb.as_ref() {
        "-h" | "--help" => {
            let _ = write!(stdout, "{SKILLS_USAGE}");
            exit::OK
        }
        "list" => list::run(rest, stdout, stderr, format),
        "status" => status::run(rest, stdout, stderr, format),
        "targets" => misc::targets(rest, stdout, stderr, format),
        "log" => misc::log(rest, stdout, stderr, format),
        "sync" => status::sync(rest, stdout, stderr, format),
        "doctor" => misc::doctor(rest, stdout, stderr, format),
        _ => {
            let _ = writeln!(
                stderr,
                "symbrain skills: unknown subcommand {}\n",
                flags::quote_argument(&args[0])
            );
            let _ = write!(stderr, "{SKILLS_USAGE}");
            exit::USAGE
        }
    })
}

pub(crate) fn resolve_skills_dirs() -> (PathBuf, PathBuf, PathBuf) {
    let cfg = symbrain_skills::config::load_cli();
    (
        cfg.library_dir,
        cfg.base_dir,
        symbrain_skills::config::home_dir(),
    )
}
fn current_project_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default()
}
fn or_dash(value: &str) -> &str {
    if value.trim().is_empty() { "-" } else { value }
}

// Reporting follows completed reads/sync: a sink failure never rolls back work.
fn report_result(verb: &str, result: std::io::Result<()>, stderr: &mut dyn Write) -> u8 {
    match result {
        Ok(()) => exit::OK,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain skills {verb}: format output: {error}");
            exit::GENERIC
        }
    }
}

#[cfg(test)]
#[path = "skills_report_tests.rs"]
mod report_tests;
