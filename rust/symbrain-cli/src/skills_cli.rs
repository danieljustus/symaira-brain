//! Native `symbrain skills` CLI.
//!
//! Every invocation is answered in process with Go's bytes: there is no
//! delegation to another implementation. The six subcommands mirror
//! `cmd/symbrain/cmd_skills.go`; their shared inputs (flag grammar, the
//! symskills configuration, home, project and log locations) live in
//! [`flags`] and [`env`].

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::PathBuf;

use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_skills::gotext::clean;

mod doctor;
mod env;
mod flags;
mod history;
mod import;
mod install;
mod list;
mod log;
mod opargs;
mod ops;
mod pull;
mod render;
mod restore;
mod status;
mod sync;
mod targets;
mod uninstall;
mod validate;
mod warn;

/// The shipped `symbrain skills` help, verbatim from the shipped source.
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

/// Resolves the skills data root once for the native `symbrain sync` verb.
///
/// Delegates to [`symbrain_skills::runner::skills_data_root`], whose branches
/// are frozen in the Go runner oracle's `defaults` scenarios: an absolute
/// `$XDG_DATA_HOME` (relative values ignored per the XDG spec), else
/// `$HOME/.local/share`; then `symbrain/skills` unless it is absent while the
/// legacy `symskills` directory exists. The fallback only covers the branch
/// the runner cannot decide — no absolute `$XDG_DATA_HOME` and no usable home
/// — and preserves the pre-parity resolution for it.
fn skills_data_root() -> PathBuf {
    if let Ok(root) = symbrain_skills::runner::skills_data_root() {
        return root;
    }
    let home = symbrain_core::xdg::home_dir().unwrap_or_else(|| PathBuf::from("."));
    if let Some(data) = symbrain_core::xdg::data_dir() {
        data.join("skills")
    } else {
        home.join(".local").join("share").join("symskills")
    }
}

/// Targets switched off by `disabled_targets` in the symskills config files
/// that `symbrain sync` shares with `symbrain skills`.
pub(crate) fn resolve_disabled_targets() -> Vec<String> {
    let global = env::config_path();
    let project = env::go_getwd().map(|dir| symbrain_skills::gotext::join(dir, ".symskills.toml"));
    let mut files: Vec<&std::path::Path> = vec![global.as_path()];
    files.extend(project.as_deref());
    symbrain_skills::disabled_targets::load(&files)
}

/// Library, base and home directories for `symbrain sync`'s skill push.
pub(crate) fn resolve_skills_dirs() -> (PathBuf, PathBuf, PathBuf) {
    let home = symbrain_core::xdg::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let data_root = skills_data_root();
    let library_dir = if let Some(path) = std::env::var_os("SYMBRAIN_SKILLS_LIBRARY_DIR") {
        PathBuf::from(path)
    } else {
        data_root.join("library")
    };
    let base_dir = if let Some(path) = std::env::var_os("SYMBRAIN_SKILLS_BASE_DIR") {
        PathBuf::from(path)
    } else {
        data_root.join("base")
    };
    (library_dir, base_dir, home)
}

/// Runs `symbrain skills` against the actual process stdout descriptor, so a
/// readonly or closed descriptor fails like Go's `os.Stdout` instead of being
/// swallowed by `std::io::Stdout`.
pub(crate) fn run_process(args: &[OsString], stderr: &mut dyn Write, format: OutputFormat) -> u8 {
    let mut raw = crate::skills_stdout::ProcessStdout;
    let mut stdout = crate::stdio_output::Output::new(&mut raw, true);
    run(args, &mut stdout, stderr, format)
}

/// Runs `symbrain skills` and returns the process exit code.
pub fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let Some(first) = args.first() else {
        let _ = write!(stderr, "{SKILLS_USAGE}");
        return exit::USAGE;
    };
    let rest = &args[1..];
    match first.to_str() {
        Some("-h" | "--help") => {
            let _ = write!(stdout, "{SKILLS_USAGE}");
            exit::OK
        }
        Some("list") => list::run(rest, stdout, stderr, format),
        Some("status") => status::run(rest, stdout, stderr, format),
        Some("targets") => targets::run(rest, stdout, stderr, format),
        Some("log") => log::run(rest, stdout, stderr, format),
        Some("sync") => sync::run(rest, stdout, stderr, format),
        Some("doctor") => doctor::run(rest, stdout, stderr, format),
        Some("import") => import::run(rest, stdout, stderr, format),
        Some("pull") => pull::run(rest, stdout, stderr, format),
        Some("install") => install::run(rest, stdout, stderr, format),
        Some("uninstall") => uninstall::run(rest, stdout, stderr, format),
        Some("render") => render::run(rest, stdout, stderr, format),
        Some("validate") => validate::run(rest, stdout, stderr, format),
        Some("history") => history::run(rest, stdout, stderr, format),
        Some("restore") => restore::run(rest, stdout, stderr, format),
        _ => {
            let _ = writeln!(
                stderr,
                "symbrain skills: unknown subcommand {}\n",
                quote_argument(first)
            );
            let _ = write!(stderr, "{SKILLS_USAGE}");
            exit::USAGE
        }
    }
}

/// Go `%q` of one raw argument.
fn quote_argument(value: &OsStr) -> String {
    symbrain_core::config::format_go_quoted_bytes(value.as_encoded_bytes())
}

/// `filepath.Clean` for a configured directory, keeping Go's empty string.
fn cleaned(path: &std::path::Path) -> PathBuf {
    if path.as_os_str().is_empty() {
        PathBuf::new()
    } else {
        clean(path)
    }
}

/// `strings.TrimSpace(value) == "" ? "-" : value`.
fn or_dash(value: &str) -> &str {
    if value.trim().is_empty() { "-" } else { value }
}

/// Renders a report the way Go's `json.Encoder` does (compact, HTML-escaped,
/// trailing newline); invalid UTF-8 in a path becomes the `\ufffd` escape.
fn json_line<T: serde::Serialize>(report: &T) -> Vec<u8> {
    let mut text = crate::go_json(report).replace(NON_UTF8_MARK, "\\ufffd");
    text.push('\n');
    text.into_bytes()
}

/// Stand-in for one invalid UTF-8 byte inside serialized text; Go's encoder
/// writes the six-character escape `\ufffd` for each such byte.
const NON_UTF8_MARK: char = '\u{10fffe}';

/// Text of a path for JSON output with [`NON_UTF8_MARK`] per invalid byte.
fn json_text(path: &std::path::Path) -> String {
    let mut out = String::new();
    for chunk in path.as_os_str().as_encoded_bytes().utf8_chunks() {
        out.push_str(chunk.valid());
        for _ in chunk.invalid() {
            out.push(NON_UTF8_MARK);
        }
    }
    out
}

fn serialize_path<S: serde::Serializer>(
    path: &std::path::Path,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&json_text(path))
}

/// Writes a finished report; a failing sink reports like Go's `renderSkills`.
fn finish(verb: &str, output: &[u8], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    match stdout.write_all(output).and_then(|()| stdout.flush()) {
        Ok(()) => exit::OK,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain skills {verb}: format output: {error}");
            exit::GENERIC
        }
    }
}
