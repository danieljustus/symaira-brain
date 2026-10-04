use std::ffi::OsString;
use std::io::Write;

use symbrain_core::{
    config::{format_go_quoted_bytes, os_bytes},
    exit,
    output::OutputFormat,
};

const DOCTOR_USAGE: &str = "Usage of doctor:\n  -fix\n    \trepair missing or version-mismatched managed binaries\n  -force-release\n    \twith --fix: allow replacing a brain-source build with the pinned release download\n  -json\n    \temit machine-readable JSON\n  -vault-agent string\n    \tvault agent name for MCP handshake probe (default \"claude-code\")\n";

#[path = "doctor_output.rs"]
mod doctor_output;

#[path = "doctor_render.rs"]
mod doctor_render;

#[path = "doctor_checks.rs"]
mod doctor_checks;
#[path = "doctor_core.rs"]
mod doctor_core;
#[path = "doctor_fix.rs"]
mod doctor_fix;
#[path = "doctor_links.rs"]
mod doctor_links;
#[path = "doctor_process.rs"]
pub(crate) mod doctor_process;
#[path = "doctor_types.rs"]
mod doctor_types;

/// Config-load diagnostics remain Go-owned until the typed loader is ported.
pub(crate) fn requires_go_fallback(args: &[OsString]) -> bool {
    parse_args(args, &mut Vec::new()).is_ok_and(|parsed| parsed.fix)
        && doctor_fix::managed_bin_dir().is_some()
        && (!crate::vault_config::valid_configuration()
            || crate::setup_cli::enabled_cores().is_err())
}

pub fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    run_with_stdout(args, stdout, stderr, format, false)
}

pub(crate) fn run_with_stdout(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
    process_stdout: bool,
) -> u8 {
    let mut output = crate::stdio_output::Output::new(stdout, process_stdout);
    let parsed = match parse_args(args, stderr) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    if parsed.fix {
        return doctor_fix::run_fix(
            parsed.force_release,
            &mut doctor_output::GoWriter::new(&mut output),
            stderr,
        );
    }
    let report = doctor_checks::run_checks(&parsed.vault_agent);
    let format = if parsed.json {
        OutputFormat::Json
    } else {
        format
    };
    match format {
        OutputFormat::Json => {
            // json.Encoder submits the complete document and newline together.
            let document = format!("{}\n", crate::go_json(&report));
            if let Err(error) = output.write_all(document.as_bytes()) {
                let _ = writeln!(
                    stderr,
                    "symbrain doctor: {}",
                    crate::stdio_output::io_cause(&error)
                );
                return exit::GENERIC;
            }
        }
        OutputFormat::Table => {
            // Go ignores individual fmt failures and continues the next line.
            // The outer Output still handles a real process-stdout EPIPE at
            // the failing write, before this writer can ignore it.
            let _ = doctor_render::human(&mut doctor_output::GoWriter::new(&mut output), &report);
        }
    }
    exit::OK
}

#[derive(Default)]
struct DoctorArgs {
    fix: bool,
    force_release: bool,
    json: bool,
    vault_agent: String,
}

fn parse_args(args: &[OsString], stderr: &mut dyn Write) -> Result<DoctorArgs, u8> {
    let mut parsed = DoctorArgs {
        vault_agent: "claude-code".to_string(),
        ..DoctorArgs::default()
    };
    let normalized = crate::normalize_flags(args);
    let mut i = 0;
    while i < normalized.len() {
        let arg = os_bytes(&normalized[i]);
        if arg.as_ref() == b"--" {
            break;
        }
        if !arg.starts_with(b"-") || arg.as_ref() == b"-" {
            break;
        }
        let flag = arg.strip_prefix(b"--").unwrap_or(&arg[1..]);
        let (name, inline) = flag
            .iter()
            .position(|byte| *byte == b'=')
            .map_or((flag, None), |at| (&flag[..at], Some(&flag[at + 1..])));
        if name.is_empty() || name.starts_with(b"-") || name.starts_with(b"=") {
            let _ = stderr.write_all(b"bad flag syntax: ");
            let _ = stderr.write_all(&arg);
            let _ = write!(stderr, "\n{DOCTOR_USAGE}");
            return Err(exit::USAGE);
        }
        match name {
            b"json" => parsed.json = parse_bool_flag(name, inline, stderr)?,
            b"force-release" => {
                parsed.force_release = parse_bool_flag(name, inline, stderr)?;
            }
            b"fix" => parsed.fix = parse_bool_flag(name, inline, stderr)?,
            b"vault-agent" => {
                let value = inline
                    .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                    .or_else(|| {
                        i += 1;
                        normalized.get(i).map(|v| v.to_string_lossy().into_owned())
                    });
                let Some(value) = value else {
                    // Go's flag package prints the parse error and then the
                    // whole flag set usage for a missing value argument.
                    let _ = writeln!(stderr, "flag needs an argument: -vault-agent");
                    let _ = write!(stderr, "{DOCTOR_USAGE}");
                    return Err(exit::USAGE);
                };
                parsed.vault_agent = value;
            }
            b"h" | b"help" => {
                let _ = write!(stderr, "{DOCTOR_USAGE}");
                return Err(exit::USAGE);
            }
            _ => {
                let _ = stderr.write_all(b"flag provided but not defined: -");
                let _ = stderr.write_all(name);
                let _ = stderr.write_all(b"\n");
                let _ = write!(stderr, "{DOCTOR_USAGE}");
                return Err(exit::USAGE);
            }
        }
        i += 1;
    }
    Ok(parsed)
}

fn parse_bool_flag(name: &[u8], value: Option<&[u8]>, stderr: &mut dyn Write) -> Result<bool, u8> {
    let Some(value) = value else { return Ok(true) };
    std::str::from_utf8(value)
        .ok()
        .and_then(|text| crate::setup_cli::parse_go_bool(text).ok())
        .ok_or_else(|| {
            let quoted = format_go_quoted_bytes(value);
            let _ = write!(stderr, "invalid boolean value {quoted} for -");
            let _ = stderr.write_all(name);
            let _ = write!(stderr, ": parse error\n{DOCTOR_USAGE}");
            exit::USAGE
        })
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::doctor_core::probe_version_with_args;
    use super::doctor_core::{parse_server_map, profile_arg, sorted_server_names};
    #[cfg(unix)]
    use super::doctor_links::classify_vault_failure;
    use super::doctor_links::is_secret_reference;
    use super::doctor_types::HARNESSES;
    use super::*;
    use std::path::Path;

    #[test]
    fn json_harness_entries_match_go_basename_and_profile_rules() {
        let spec = HARNESSES[0];
        let map = parse_server_map(
            spec,
            br#"{"mcpServers":{"old":{"command":"/opt/symmemory"},"symbrain":{"command":"/opt/symbrain","args":["mcp","--profile=work"]}}}"#,
        )
        .expect("valid harness JSON");
        assert_eq!(sorted_server_names(&map), vec!["old", "symbrain"]);
        assert_eq!(
            profile_arg(map["symbrain"].get("args").unwrap().as_array().unwrap()),
            Some("work".to_string())
        );
        let old = map.get("old").unwrap();
        assert_eq!(
            Path::new(old.get("command").unwrap().as_str().unwrap())
                .file_name()
                .unwrap(),
            "symmemory"
        );
    }

    #[test]
    fn toml_harness_entries_preserve_sorted_server_names() {
        let spec = HARNESSES[4];
        let map = parse_server_map(
            spec,
            br#"[mcp_servers.zed]
command = "zed"
args = ["--x"]
[mcp_servers.symbrain]
command = "symbrain"
args = ["mcp", "--profile", "default"]
"#,
        )
        .expect("valid harness TOML");
        assert_eq!(sorted_server_names(&map), vec!["symbrain", "zed"]);
        assert_eq!(
            profile_arg(map["symbrain"].get("args").unwrap().as_array().unwrap()),
            Some("default".to_string())
        );
    }

    #[test]
    fn secret_reference_classification_is_compatible_with_go() {
        assert!(is_secret_reference("symvault://memory/jwt"));
        assert!(is_secret_reference("vault://memory/jwt"));
        assert!(is_secret_reference("env://SYMBRAIN_JWT"));
        assert!(is_secret_reference("keychain://symbrain/account"));
        assert!(!is_secret_reference("literal-secret"));
    }

    #[cfg(unix)]
    #[test]
    fn arbitrary_vault_stderr_is_not_exposed() {
        let status = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 42"])
            .status()
            .expect("exit status");
        let report = classify_vault_failure(
            "vault: reachable".to_string(),
            status,
            b"credential=SENTINEL_SECRET",
        );
        let encoded = serde_json::to_string(&report).expect("JSON");
        assert!(!encoded.contains("SENTINEL_SECRET"));
        assert_eq!(report.detail, "symvault probe failed: exit status 42");
    }

    #[cfg(unix)]
    #[test]
    fn nonzero_version_probe_cannot_be_accepted() {
        let error = probe_version_with_args(
            Path::new("/bin/sh"),
            &["-c", "printf '{\"version\":\"9.9.9\"}'; exit 42", "probe"],
        )
        .expect_err("nonzero probe");
        assert!(error.contains("exit status 42"));
    }

    #[test]
    fn doctor_argument_parser_accepts_go_flag_forms() {
        let args = vec![
            OsString::from("--fix=false"),
            OsString::from("--force-release=false"),
            OsString::from("--vault-agent"),
            OsString::from("agent"),
        ];
        let parsed = parse_args(&args, &mut Vec::new()).expect("valid flags");
        assert!(!parsed.fix);
        assert_eq!(parsed.vault_agent, "agent");
    }

    #[test]
    fn valid_configuration_allows_native_repair_flag_forms() {
        let args = |rest: &[&str]| -> Vec<OsString> { rest.iter().map(OsString::from).collect() };
        assert!(!requires_go_fallback(&args(&["--vault-agent", "agent"])));
        assert!(!requires_go_fallback(&args(&["--vault-agent=agent"])));
        assert!(!requires_go_fallback(&args(&["--json"])));
        assert!(!requires_go_fallback(&args(&["--force-release"])));
        assert!(!requires_go_fallback(&args(&["--force-release=true"])));
        assert!(!requires_go_fallback(&args(&["--fix"])));
        assert!(!requires_go_fallback(&args(&["--fix=TRUE"])));
        assert!(!requires_go_fallback(&args(&["--fix=0"])));
        assert!(!requires_go_fallback(&args(&["--force-release", "--fix"])));
        assert!(!requires_go_fallback(&args(&[
            "--fix=false",
            "--force-release=false"
        ])));
        assert!(!requires_go_fallback(&args(&["--fix=FALSE"])));
    }

    #[test]
    fn help_lists_the_force_release_flag() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run(
            &[OsString::from("--help")],
            &mut stdout,
            &mut stderr,
            OutputFormat::Table,
        );

        assert_eq!(code, exit::USAGE);
        assert!(stdout.is_empty());
        assert_eq!(stderr, DOCTOR_USAGE.as_bytes());
    }
}
