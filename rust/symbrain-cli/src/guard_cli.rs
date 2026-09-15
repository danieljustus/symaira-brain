//! Native implementation of the `symbrain guard decide` one-shot route.

use std::env;
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset};
use symbrain_audit::RawJsonlAppender;
use symbrain_core::exit;
use symbrain_guard_core::external_decision::{
    MAX_REQUEST_BYTES, evaluate_at, evaluate_read_error_at,
};
use symbrain_guard_core::go_json::to_go_json_vec;

/// Runs `symbrain guard` and keeps every verb except `decide` on the Go fallback.
pub fn run(args: &[OsString], stdout: &mut dyn Write, _stderr: &mut dyn Write) -> Option<u8> {
    let verb = args.first().map(|arg| arg.as_os_str().to_string_lossy())?;
    if verb != "decide" {
        return None;
    }
    let rest = &args[1..];
    if rest.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Some(write_help(stdout));
    }
    // The pinned Go Run checks help and otherwise ignores all arguments.

    let stdin = io::stdin();
    let mut input = stdin.lock();
    Some(run_at_path(
        &mut input,
        stdout,
        audit_path(),
        chrono::Utc::now().fixed_offset(),
    ))
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
}
