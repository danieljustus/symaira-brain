use std::ffi::OsString;
use std::io::Write;

use symbrain_core::{exit, output::OutputFormat};

const DOCTOR_USAGE: &str = "Usage of doctor:\n  -fix\n    \trepair missing or version-mismatched managed binaries\n  -json\n    \temit machine-readable JSON\n  -vault-agent string\n    \tvault agent name for MCP handshake probe (default \"claude-code\")\n";

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
mod doctor_process;
#[path = "doctor_types.rs"]
mod doctor_types;

pub fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let parsed = match parse_args(args, stderr) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    if parsed.fix {
        return doctor_fix::run_fix(stdout, stderr);
    }
    let report = doctor_checks::run_checks(&parsed.vault_agent);
    let result = match format {
        OutputFormat::Json => symbrain_core::output::render_json(&mut *stdout, &report),
        OutputFormat::Table => doctor_render::human(stdout, &report),
    };
    if result.is_err() {
        let _ = writeln!(stderr, "symbrain doctor: format output");
        exit::GENERIC
    } else {
        exit::OK
    }
}

#[derive(Default)]
struct DoctorArgs {
    fix: bool,
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
        let arg = normalized[i].to_string_lossy();
        if arg == "--" {
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            break;
        }
        let flag = arg.trim_start_matches('-');
        let (name, inline) = flag
            .split_once('=')
            .map_or((flag, None), |(n, v)| (n, Some(v)));
        match name {
            "json" => {}
            "fix" => parsed.fix = inline != Some("false"),
            "vault-agent" => {
                let value = inline.map(str::to_owned).or_else(|| {
                    i += 1;
                    normalized.get(i).map(|v| v.to_string_lossy().into_owned())
                });
                let Some(value) = value else {
                    let _ = writeln!(stderr, "flag needs an argument: -vault-agent");
                    return Err(exit::USAGE);
                };
                parsed.vault_agent = value;
            }
            "h" | "help" => {
                let _ = write!(stderr, "{DOCTOR_USAGE}");
                return Err(exit::USAGE);
            }
            _ => {
                let _ = writeln!(stderr, "flag provided but not defined: -{name}");
                let _ = write!(stderr, "{DOCTOR_USAGE}");
                return Err(exit::USAGE);
            }
        }
        i += 1;
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::doctor_core::{
        parse_server_map, probe_version_with_args, profile_arg, sorted_server_names,
    };
    use super::doctor_links::{classify_vault_failure, is_secret_reference};
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
            OsString::from("--vault-agent"),
            OsString::from("agent"),
        ];
        let parsed = parse_args(&args, &mut Vec::new()).expect("valid flags");
        assert!(!parsed.fix);
        assert_eq!(parsed.vault_agent, "agent");
    }
}
