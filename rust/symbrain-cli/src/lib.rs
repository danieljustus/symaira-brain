#![deny(unsafe_code)]

use std::env;
use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use symbrain_core::exit;
use symbrain_core::output::{self, OutputFormat};
use symbrain_core::version::{self, VersionInfo};
use symbrain_core::xdg;

mod audit_cli;
mod doctor_cli;
mod install_cli;
mod mcp_cli;
mod passthrough;
mod profile_actions;
mod profile_args;
mod profile_cli;
mod profile_render;
mod setup_cli;
mod usage_cli;

const USAGE: &str = "symbrain — portable agent-context layer for AI harnesses\n\nUsage:\n  symbrain <command> [flags]\n\nGlobal output flags (version, sync, memory, skills, activity, profile, harness, audit, usage, and doctor):\n  --output table|json  Output format (default: table)\n  --json               Shorthand for --output json\n\nCommands:\n  init        Create XDG directories, default config, and example profiles\n  doctor      Check environment, config, profiles, and child binaries\n  setup       Download and install pinned core binaries to ~/.symaira/bin\n  profile     Manage profiles (list, show, add, remove)\n  config      Inspect and edit the global config (path, get, set)\n  harness     Inspect registered AI harnesses and their MCP servers\n  usage       AI subscription/token usage per provider\n  mcp         Run the MCP gateway over stdio for a profile (serve is a deprecated alias)\n  install     Register symbrain with a harness\n  uninstall   Remove symbrain from a harness\n  sync        Sync instructions and skills to harnesses\n  memory      Operate the embedded memory store (list, search, set, delete, rules, query-log, sync, serve)\n  skills      Operate the embedded skill library (list, status, targets, log, sync, doctor)\n  activity    Read bounded activity summaries with explicit profile access\n  audit       Inspect the audit log\n  vault       Passthrough to symvault\n  guard       Absorbed symguard commands (decide, scan, doctor, grants, version)\n\n  version     Print version information\n  help        Show this help message\n\nRun 'symbrain <command> --help' for details on a specific command.\n";

/// Execution strategy for commands delegated to the Go oracle or child processes.
pub trait FallbackExecutor {
    /// Executes an unmigrated command with the provided arguments and diagnostics writer.
    fn execute(&self, args: &[OsString], stderr: &mut dyn Write) -> u8;
}

/// Production executor that spawns the Go fallback child process with inherited stdio.
///
/// Inherited stdio preserves interactive streaming for protocols like MCP (JSON-RPC over
/// stdin/stdout) and prevents buffering or stdio pollution across process boundaries.
pub struct InheritedProcessExecutor;

impl FallbackExecutor for InheritedProcessExecutor {
    fn execute(&self, args: &[OsString], stderr: &mut dyn Write) -> u8 {
        run_go_fallback(args, stderr)
    }
}

/// Normalizes CLI arguments matching Go's `normalizeFlags`:
/// - Converts `--flag` to `-flag` when length > 2
/// - Preserves positional arguments and bare `-`
/// - Preserves `--` and all arguments following `--`
#[must_use]
pub fn normalize_flags(args: &[OsString]) -> Vec<OsString> {
    let mut out = Vec::with_capacity(args.len());
    let mut terminated = false;
    for arg in args {
        if terminated {
            out.push(arg.clone());
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::{OsStrExt, OsStringExt};
            let bytes = arg.as_os_str().as_bytes();
            if bytes == b"--" {
                terminated = true;
                out.push(arg.clone());
            } else if bytes.starts_with(b"--") && bytes.len() > 2 {
                let mut normalized = Vec::with_capacity(bytes.len() - 1);
                normalized.push(b'-');
                normalized.extend_from_slice(&bytes[2..]);
                out.push(OsString::from_vec(normalized));
            } else {
                out.push(arg.clone());
            }
        }
        #[cfg(not(unix))]
        {
            let s = arg.to_string_lossy();
            if s == "--" {
                terminated = true;
                out.push(arg.clone());
            } else if s.starts_with("--") && s.len() > 2 {
                out.push(OsString::from(format!("-{}", &s[2..])));
            } else {
                out.push(arg.clone());
            }
        }
    }
    out
}

/// Runs `symbrain` with the production inherited-process fallback executor.
pub fn run(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    if args.first().is_some_and(|arg| arg == "vault") {
        return passthrough::run(args, stderr);
    }
    run_with_executor(args, stdout, stderr, &InheritedProcessExecutor)
}

/// Runs `symbrain` with an explicit fallback executor strategy.
pub fn run_with_executor<E: FallbackExecutor>(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    executor: &E,
) -> u8 {
    if let Some(code) = run_in_process(args, stdout, stderr) {
        code
    } else {
        executor.execute(args, stderr)
    }
}

/// Attempts to execute a command natively in-process without invoking any fallback.
///
/// Returns `Some(code)` if the command was recognized and handled in-process, writing
/// all output directly to `stdout` and `stderr`.
/// Returns `None` if the command is unmigrated and requires fallback execution.
pub fn run_in_process(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Option<u8> {
    let peeked = peek_command(args);
    let (format, normalized) = if is_output_command(&peeked) {
        match output::extract_format(args) {
            Ok(result) => result,
            Err(err) => {
                let _ = writeln!(stderr, "symbrain: {err}");
                return Some(exit::USAGE);
            }
        }
    } else {
        (OutputFormat::Table, args.to_vec())
    };

    if normalized.is_empty() {
        if write!(stdout, "{USAGE}").is_err() {
            return Some(exit::GENERIC);
        }
        return Some(exit::USAGE);
    }

    let cmd = normalized[0].to_string_lossy();
    let rest = &normalized[1..];

    match cmd.as_ref() {
        "help" | "--help" | "-h" => Some(write_usage(stdout)),
        "version" => Some(run_version(rest, stdout, stderr, format)),
        "config" => run_config(rest, stdout, stderr),
        "profile" => profile_cli::run(rest, stdout, stderr, format),
        "audit" => Some(audit_cli::run(rest, stdout, stderr, format)),
        "setup" => Some(setup_cli::run(rest, stdout, stderr)),
        "doctor" => Some(doctor_cli::run(rest, stdout, stderr, format)),
        "install" => Some(install_cli::run_install(rest, stdout, stderr)),
        "uninstall" => Some(install_cli::run_uninstall(rest, stdout, stderr)),
        "mcp" => Some(mcp_cli::run(rest, stderr)),
        "serve" => Some(mcp_cli::run_serve(rest, stderr)),
        "usage" => Some(usage_cli::run(rest, stdout, stderr, format)),
        "init" | "harness" | "sync" | "memory" | "skills" | "activity" | "vault" | "guard" => None,
        _ => {
            let _ = writeln!(stderr, "symbrain: unknown command {cmd:?}\n");
            let _ = write!(stderr, "{USAGE}");
            Some(exit::USAGE)
        }
    }
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

fn run_config(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> Option<u8> {
    let normalized = normalize_flags(args);
    let (subcommand, sub_args) = if normalized.first().is_some_and(|a| a == "--") {
        let sub = normalized.get(1).map(|s| s.to_string_lossy());
        let sub_args = if normalized.len() > 2 {
            &normalized[2..]
        } else {
            &[]
        };
        (sub, sub_args)
    } else {
        let sub = normalized.first().map(|s| s.to_string_lossy());
        let sub_args = if normalized.len() > 1 {
            &normalized[1..]
        } else {
            &[]
        };
        (sub, sub_args)
    };

    match subcommand.as_deref() {
        Some("path") => Some(run_config_path(sub_args, stdout)),
        Some("get") => Some(symbrain_core::config::run_config_get(
            sub_args, stdout, stderr,
        )),
        Some("set") => Some(symbrain_core::config::run_config_set(
            sub_args, stdout, stderr,
        )),
        _ => None,
    }
}

fn run_config_path(args: &[OsString], stdout: &mut dyn Write) -> u8 {
    if let Some(unexpected) = args.first() {
        let _ = writeln!(
            stdout,
            "symbrain config path: unexpected argument {:?}",
            unexpected.to_string_lossy()
        );
        return exit::USAGE;
    }
    let path = xdg::config_path();
    if writeln!(stdout, "{}", path.display()).is_ok() {
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

#[cfg(unix)]
fn exit_status_code(status: std::process::ExitStatus) -> u8 {
    use std::os::unix::process::ExitStatusExt;
    if let Some(code) = status.code() {
        u8::try_from(code).unwrap_or(exit::GENERIC)
    } else if let Some(sig) = status.signal() {
        u8::try_from(128 + sig).unwrap_or(exit::GENERIC)
    } else {
        exit::GENERIC
    }
}

#[cfg(not(unix))]
fn exit_status_code(status: std::process::ExitStatus) -> u8 {
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .unwrap_or(exit::GENERIC)
}

fn run_go_fallback(args: &[OsString], stderr: &mut dyn Write) -> u8 {
    let Some(binary) = go_fallback_binary() else {
        let command = args
            .first()
            .map_or_else(|| "".into(), |arg| arg.to_string_lossy());
        let _ = writeln!(
            stderr,
            "symbrain: command {command:?} is not ported yet and no Go fallback was found; set SYMBRAIN_GO_BINARY"
        );
        return exit::GENERIC;
    };

    let status = Command::new(&binary)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
    match status {
        Ok(status) => exit_status_code(status),
        Err(error) => {
            let _ = writeln!(
                stderr,
                "symbrain: start Go fallback {}: {error}",
                binary.display()
            );
            exit::GENERIC
        }
    }
}

fn go_fallback_binary() -> Option<PathBuf> {
    if let Some(path) = env::var_os("SYMBRAIN_GO_BINARY") {
        return Some(PathBuf::from(path));
    }
    path_lookup(OsStr::new("symbrain-go"))
}

fn path_lookup(binary: &OsStr) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|dir| dir.join(binary))
            .find(|candidate| candidate.is_file())
    })
}

fn rustc_version() -> &'static str {
    option_env!("RUSTC_VERSION").unwrap_or("rustc")
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
