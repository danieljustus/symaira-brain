//! Native implementation of the `symbrain guard decide` one-shot route.

use std::env;
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset};
use symbrain_audit::RawJsonlAppender;
use symbrain_core::exit;
use symbrain_core::version;
use symbrain_guard_core::external_decision::{
    MAX_REQUEST_BYTES, evaluate_at, evaluate_read_error_at,
};
use symbrain_guard_core::go_json::to_go_json_vec;

const GUARD_USAGE: &str = r"symguard — local-first security gateway (absorbed into symbrain)

Usage:
  symbrain guard <command> [flags]

Commands:
  version   Print version and build info
  doctor    Check system health and configuration
  decide    Read a JSON decision request from stdin, write the decision to stdout
  grants    List and revoke standing grants
  scan      Discover MCP servers across supported AI clients
  help      Show this help message

Run 'symbrain guard <command> --help' for details on a specific command.";

#[path = "guard_grants.rs"]
mod guard_grants;
#[path = "guard_scan.rs"]
mod guard_scan;

/// Runs `symbrain guard`.
pub fn run(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> Option<u8> {
    if args.is_empty() {
        return Some(write_guard_usage(stdout, exit::USAGE));
    }
    let verb = args.first().map(|arg| arg.as_os_str().to_string_lossy())?;
    match verb.as_ref() {
        "help" | "--help" | "-h" => Some(write_guard_usage(stdout, exit::OK)),
        "version" => {
            // The shipped `symguard version` prints a four-row block: its own
            // version, the toolchain that built it, the platform, and a
            // hard-coded build-time placeholder (`buildTime()` parses a
            // constant date rather than recording a real one, so the port
            // reproduces it verbatim instead of inventing a timestamp). The
            // toolchain row names Rust honestly; the frozen expectation
            // tokenizes that row, because no runner can pin a toolchain.
            let version = option_env!("SYMBRAIN_VERSION").unwrap_or("dev");
            let info = version::VersionInfo::new("symguard", version);
            if args[1..].iter().any(|arg| arg == "--json") {
                let _ = writeln!(
                    stdout,
                    "{}",
                    serde_json::to_string_pretty(&info).unwrap_or_default()
                );
            } else {
                let _ = writeln!(stdout, "symguard {version}");
                let _ = writeln!(stdout, "  rust    {}", crate::rustc_version());
                let _ = writeln!(
                    stdout,
                    "  os/arch {}/{}",
                    version::current_os(),
                    version::current_arch()
                );
                let _ = writeln!(stdout, "  built   2026-01-01 (compile-time placeholder)");
            }
            Some(exit::OK)
        }
        "decide" => {
            let rest = &args[1..];
            if rest.iter().any(|arg| arg == "--help" || arg == "-h") {
                return Some(write_help(stdout));
            }
            let stdin = io::stdin();
            let mut input = stdin.lock();
            Some(run_at_path(
                &mut input,
                stdout,
                audit_path(),
                chrono::Utc::now().fixed_offset(),
            ))
        }
        "scan" => Some(guard_scan::run(&args[1..], stdout, stderr)),
        "grants" => Some(guard_grants::run(&args[1..], stdout, stderr)),
        "doctor" => None,
        _ => {
            let _ = writeln!(stderr, "symbrain guard: unknown command {verb:?}\n");
            let _ = writeln!(stderr, "{GUARD_USAGE}");
            Some(exit::USAGE)
        }
    }
}

fn write_guard_usage(stdout: &mut dyn Write, code: u8) -> u8 {
    let _ = writeln!(stdout, "{GUARD_USAGE}");
    code
}

/// Executes the production decide boundary with explicit clock and audit path.
///
/// This is public so integration tests exercise the same implementation used
/// by the binary rather than a test-only copy of the adapter.
pub fn run_at_path<R: Read, W: Write>(
    mut input: R,
    mut output: W,
    path: PathBuf,
    now: DateTime<FixedOffset>,
) -> u8 {
    let mut bytes = Vec::with_capacity(MAX_REQUEST_BYTES + 1);
    let read_result = input
        .by_ref()
        .take((MAX_REQUEST_BYTES + 1) as u64)
        .read_to_end(&mut bytes);

    let appender = RawJsonlAppender::new(path);
    let mut sink = |record: &symbrain_guard_core::external_decision::ExternalDecisionAudit| {
        let serialized = to_go_json_vec(record)
            .map_err(|error| format!("decide: marshal audit record: {error}"))?;
        appender
            .append(&serialized)
            .map_err(|error| format!("decide: write audit log: {error}"))
    };

    let response = match read_result {
        Ok(_) => evaluate_at(&bytes, now, &mut sink),
        Err(error) => evaluate_read_error_at(error, now, &mut sink),
    };
    let Ok(mut encoded) = to_go_json_vec(&response) else {
        return exit::GENERIC;
    };
    encoded.push(b'\n');
    if output.write_all(&encoded).is_ok() {
        exit::OK
    } else {
        exit::GENERIC
    }
}

fn audit_path() -> PathBuf {
    let data_home = env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty());
    // Match Go os.UserHomeDir, not the Unix shell's HOME on Windows.
    let home_key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let home = env::var_os(home_key).filter(|value| !value.is_empty());
    audit_path_from(data_home, home, &env::temp_dir())
}

fn audit_path_from(data_home: Option<OsString>, home: Option<OsString>, temp: &Path) -> PathBuf {
    if let Some(data_home) = data_home {
        return PathBuf::from(data_home).join("symguard").join("audit.log");
    }
    if let Some(home) = home {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("symguard")
            .join("audit.log");
    }
    temp.join("symguard").join("audit.log")
}

fn write_help(stdout: &mut dyn Write) -> u8 {
    // Preserve the historical command spelling and Go's ignored help-write error.
    let _ = writeln!(
        stdout,
        r#"Usage:
  symguard decide < request.json

Reads one JSON decision request from stdin and writes the JSON decision
to stdout:

  request:  {{"command": "...", "risk_class": "low|medium|high|critical",
             "domain": "...", "warnings": ["..."]}}
  response: {{"decision": "allow|confirm|deny", "reason": "..."}}

Any error produces decision "deny" with an explanatory reason."#
    );
    exit::OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_path_prefers_xdg_then_home_then_temp() {
        let temp = PathBuf::from("/tmp/adapter-temp");
        assert_eq!(
            audit_path_from(Some("/xdg".into()), Some("/home".into()), &temp),
            PathBuf::from("/xdg/symguard/audit.log")
        );
        assert_eq!(
            audit_path_from(None, Some("/home".into()), &temp),
            PathBuf::from("/home/.local/share/symguard/audit.log")
        );
        assert_eq!(
            audit_path_from(None, None, &temp),
            PathBuf::from("/tmp/adapter-temp/symguard/audit.log")
        );
    }

    #[test]
    fn decide_preserves_go_trailing_json_diagnostic() {
        let data = tempfile::tempdir().expect("tempdir");
        let mut output = Vec::new();
        let code = run_at_path(
            br#"{"command":"open"}x"#.as_slice(),
            &mut output,
            data.path().join("symguard/audit.log"),
            "2026-09-14T12:00:00Z".parse().expect("fixed time"),
        );
        assert_eq!(code, exit::OK);
        assert_eq!(
            output,
            br#"{"decision":"deny","reason":"decide: parse request: invalid character 'x' after top-level value"}
"#
        );
    }

    #[test]
    fn unknown_verb_matches_go_dispatch() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run(&[OsString::from("frobnicate")], &mut stdout, &mut stderr);
        assert_eq!(code, Some(exit::USAGE));
        assert!(stdout.is_empty());
        assert_eq!(
            String::from_utf8(stderr).expect("utf8"),
            "symbrain guard: unknown command \"frobnicate\"\n\nsymguard — local-first security gateway (absorbed into symbrain)\n\nUsage:\n  symbrain guard <command> [flags]\n\nCommands:\n  version   Print version and build info\n  doctor    Check system health and configuration\n  decide    Read a JSON decision request from stdin, write the decision to stdout\n  grants    List and revoke standing grants\n  scan      Discover MCP servers across supported AI clients\n  help      Show this help message\n\nRun 'symbrain guard <command> --help' for details on a specific command.\n"
        );
    }

    #[test]
    fn top_level_guard_routes_match_go() {
        for (args, expected_code) in [
            (Vec::new(), exit::USAGE),
            (vec![OsString::from("help")], exit::OK),
            (vec![OsString::from("--help")], exit::OK),
            (vec![OsString::from("-h")], exit::OK),
        ] {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            assert_eq!(run(&args, &mut stdout, &mut stderr), Some(expected_code));
            assert_eq!(
                String::from_utf8(stdout).expect("utf8"),
                format!("{GUARD_USAGE}\n")
            );
            assert!(stderr.is_empty());
        }
    }

    #[test]
    fn doctor_remains_on_go_fallback() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            run(&[OsString::from("doctor")], &mut stdout, &mut stderr),
            None
        );
    }
}
