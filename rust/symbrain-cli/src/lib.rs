#![deny(unsafe_code)]

use std::ffi::OsString;
use std::io::{self, Write};
use symbrain_core::exit;
use symbrain_core::output::{self, OutputFormat};
use symbrain_core::version::{self, VersionInfo};

mod activity_cli;
mod audit_cli;
mod cli_flags;
mod config_cli;
mod stdio_output;

pub use cli_flags::normalize_flags;
mod doctor_cli;
mod go_flags;
mod go_json_escape;
pub mod guard_cli;
mod harness_cli;
mod health_probe;
mod hermes_config;
mod init_cli;
mod install_cli;
mod managed_home;
mod mcp_cli;
mod memory_cli;
mod passthrough;
mod profile_actions;
mod profile_args;
mod profile_cli;
mod profile_render;
mod setup_cli;
mod skills_cli;
mod skills_stdout;
mod sync_cli;
mod usage_cli;
mod vault_admin;
mod vault_config;

const USAGE: &str = "symbrain — portable agent-context layer for AI harnesses\n\nUsage:\n  symbrain <command> [flags]\n\nGlobal output flags (version, sync, memory, skills, activity, profile, harness, audit, usage, and doctor):\n  --output table|json  Output format (default: table)\n  --json               Shorthand for --output json\n\nCommands:\n  init        Create XDG directories, default config, and example profiles\n  doctor      Check environment, config, profiles, and child binaries\n  setup       Download and install pinned core binaries to ~/.symaira/bin\n  profile     Manage profiles (list, show, add, remove)\n  config      Inspect and edit the global config (path, get, set)\n  harness     Inspect registered AI harnesses and their MCP servers\n  usage       AI subscription/token usage per provider\n  mcp         Run the MCP gateway over stdio for a profile (serve is a deprecated alias)\n  install     Register symbrain with a harness\n  uninstall   Remove symbrain from a harness\n  sync        Sync instructions and skills to harnesses\n  memory      Operate the embedded memory store (list, search, set, delete, rules, query-log, sync, serve)\n  skills      Operate the embedded skill library (list, status, targets, log, sync, doctor)\n  activity    Read bounded activity summaries with explicit profile access\n  audit       Inspect the audit log\n  vault       Human credential management (create <path> and set <path.field> read single-line secrets from stdin; delete requires --yes)\n  guard       Absorbed symguard commands (decide, scan, doctor, grants, version)\n\n  version     Print version information\n  help        Show this help message\n\nVault approval passthrough:\n  symbrain vault approval list [--output json]\n  symbrain vault approval decide <request-id> --approve|--deny\n\nRun 'symbrain <command> --help' for details on a specific command.\n";

/// Runs `symbrain` with in-memory writers. Every command is native.
pub fn run(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    run_native(args, stdout, stderr, false)
}

/// Runs the CLI on its actual process standard streams.
///
/// Native Setup writes and Memory Set completion use this stdout identity to
/// retain Go's Unix broken-pipe termination. Embedded writers keep ordinary errors.
#[must_use]
pub fn run_stdio(args: &[OsString]) -> u8 {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    run_native(args, &mut stdout, &mut stderr, true)
}

fn run_native(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    process_stdout: bool,
) -> u8 {
    let peeked = peek_command(args);
    let (format, normalized) = if is_output_command(&peeked) {
        match output::extract_format(args) {
            Ok(result) => result,
            Err(err) => {
                let _ = writeln!(stderr, "symbrain: {err}");
                return exit::USAGE;
            }
        }
    } else {
        (OutputFormat::Table, args.to_vec())
    };

    if normalized.is_empty() {
        if write!(stdout, "{USAGE}").is_err() {
            return exit::GENERIC;
        }
        return exit::USAGE;
    }

    let cmd = normalized[0].to_string_lossy();
    let rest = &normalized[1..];

    match cmd.as_ref() {
        "help" | "--help" | "-h" => write_usage(stdout),
        "version" => run_version(rest, stdout, stderr, format),
        "config" => config_cli::run(rest, stdout, stderr),
        "profile" => profile_cli::run(rest, stdout, stderr, format),
        "audit" => audit_cli::run(rest, stdout, stderr, format),
        "setup" if process_stdout => setup_cli::run_with_stdout(rest, stdout, stderr, true),
        "setup" => setup_cli::run(rest, stdout, stderr),
        "doctor" if process_stdout => {
            doctor_cli::run_with_stdout(rest, stdout, stderr, format, true)
        }
        "doctor" => doctor_cli::run(rest, stdout, stderr, format),
        "install" => install_cli::run_install(rest, stdout, stderr),
        "uninstall" => install_cli::run_uninstall(rest, stdout, stderr),
        "mcp" => mcp_cli::run(rest, stderr),
        "serve" => mcp_cli::run_serve(rest, stderr),
        "usage" => usage_cli::run(rest, stdout, stderr, format),
        "init" => init_cli::run(rest, stdout, stderr),
        "harness" => harness_cli::run(rest, stdout, stderr, format),
        "sync" => sync_cli::run(rest, stdout, stderr, format),
        "memory" if memory_cli::is_sync(rest) => memory_cli::run_sync(rest, stdout, stderr, format),
        "memory" if process_stdout => {
            memory_cli::run_with_stdout(rest, stdout, stderr, format, true)
        }
        "memory" => memory_cli::run(rest, stdout, stderr, format),
        "skills" if process_stdout => skills_cli::run_process(rest, stderr, format),
        "skills" => skills_cli::run(rest, stdout, stderr, format),
        "activity" => activity_cli::run(rest, stdout, stderr, format),
        "vault" => vault_admin::run(args, stdout, stderr),
        "guard" => guard_cli::run(rest, stdout, stderr).unwrap_or(exit::USAGE),
        _ => {
            let _ = writeln!(stderr, "symbrain: unknown command {cmd:?}\n");
            let _ = write!(stderr, "{USAGE}");
            exit::USAGE
        }
    }
}

/// Encodes JSON the way Go's `json.Encoder` does: compact output with `&`,
/// `<`, `>`, U+2028 and U+2029 escaped, and no trailing newline.
///
/// Go renders every CLI report through `encoding/json`, so a report that must
/// match its bytes has to escape those characters too. They can only occur
/// inside string content, so a global replace never touches structure.
pub(crate) fn go_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .unwrap_or_default()
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn is_output_command(cmd: &str) -> bool {
    matches!(
        cmd,
        "version"
            | "sync"
            | "memory"
            | "skills"
            | "activity"
            | "profile"
            | "harness"
            | "audit"
            | "doctor"
            | "usage"
    )
}

fn peek_command(args: &[OsString]) -> std::borrow::Cow<'_, str> {
    let mut skip_value = false;
    for arg in args {
        if skip_value {
            skip_value = false;
            continue;
        }
        let arg = arg.to_string_lossy();
        match arg.as_ref() {
            "--json" | "-json" => {}
            "--output" | "-output" => {
                skip_value = true;
            }
            value if value.starts_with("--output=") || value.starts_with("-output=") => {}
            value if !value.starts_with('-') => return arg,
            _ => {}
        }
    }
    "".into()
}

fn write_usage(stdout: &mut dyn Write) -> u8 {
    if write!(stdout, "{USAGE}").is_ok() {
        exit::OK
    } else {
        exit::GENERIC
    }
}

fn run_version(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let normalized = normalize_flags(args);
    if let Some(first) = normalized.first() {
        let first_str = first.to_string_lossy();
        if first_str == "--" {
            if normalized.len() > 1 {
                let unexpected = args
                    .get(1)
                    .map_or_else(|| "".into(), |a| a.to_string_lossy());
                let _ = writeln!(
                    stderr,
                    "symbrain version: unexpected argument {unexpected:?}"
                );
                return exit::USAGE;
            }
        } else if first_str == "-" {
            let _ = writeln!(stderr, "symbrain version: unexpected argument \"-\"");
            return exit::USAGE;
        } else if first_str.starts_with('-') {
            let trimmed = first_str.trim_start_matches('-');
            let name = match trimmed.split_once('=') {
                Some((k, _)) => k,
                None => trimmed,
            };
            if name == "h" || name == "help" {
                let _ = writeln!(stderr, "Usage of version:");
                return exit::USAGE;
            }
            let _ = writeln!(stderr, "flag provided but not defined: -{name}");
            let _ = writeln!(stderr, "Usage of version:");
            return exit::USAGE;
        } else {
            let _ = writeln!(
                stderr,
                "symbrain version: unexpected argument {first_str:?}"
            );
            return exit::USAGE;
        }
    }

    let version = option_env!("SYMBRAIN_VERSION").unwrap_or("dev");
    let info = VersionInfo::new("symbrain", version);
    if output::render(&mut *stdout, format, &info, |w| -> io::Result<()> {
        writeln!(w, "symbrain {version}")?;
        writeln!(w, "  rust    {}", rustc_version())?;
        writeln!(
            w,
            "  os/arch {}/{}",
            version::current_os(),
            version::current_arch()
        )
    })
    .is_err()
    {
        let _ = writeln!(stderr, "symbrain version: format output");
        return exit::GENERIC;
    }
    exit::OK
}

pub(crate) fn rustc_version() -> &'static str {
    option_env!("RUSTC_VERSION").unwrap_or("rustc")
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
