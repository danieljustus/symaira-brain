use std::ffi::OsString;
use std::io::Write;

use symbrain_core::{exit, output::OutputFormat};

const DOCTOR_USAGE: &str = "Usage of doctor:\n  -fix\n    \trepair missing or version-mismatched managed binaries\n  -force-release\n    \twith --fix: allow replacing a brain-source build with the pinned release download\n  -json\n    \temit machine-readable JSON\n  -vault-agent string\n    \tvault agent name for MCP handshake probe (default \"claude-code\")\n";

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

/// Whether this invocation requires the Go implementation's lifecycle or handshake semantics.
///
/// The Rust doctor implementation intentionally does not manage source-build
/// provenance. Keep enabled `--fix` and `--force-release` in Go, where the
/// managed installer owns that behavior.
pub(crate) fn requires_go_fallback(args: &[OsString]) -> bool {
    requires_go_fallback_with(args, xdg_profiles_require_go)
}

/// Variant of [`requires_go_fallback`] with the profile precondition injected.
///
/// The `--vault-agent` decision depends on the profiles that exist on this
/// machine, which must not leak into unit tests: the case under test has to
/// state its own precondition instead of inheriting the developer's home
/// directory.
pub(crate) fn requires_go_fallback_with(
    args: &[OsString],
    profiles_require_go: impl FnOnce() -> bool,
) -> bool {
    let lifecycle_fallback = crate::has_go_owned_flag(
        args,
        &["json", "fix", "force-release", "vault-agent", "h", "help"],
        &["vault-agent"],
        &["force-release"],
        &["fix"],
    );
    if lifecycle_fallback {
        return true;
    }

    // A vault-agent only affects profile handshakes. With no profiles there
    // is no handshake to customize; unreadable profile state stays on Go.
    vault_agent_with_profiles_requires_go(args, profiles_require_go)
}

/// Reports whether the XDG profile directory holds at least one profile.
///
/// An unreadable directory fails closed onto the Go fallback.
fn xdg_profiles_require_go() -> bool {
    match symbrain_policy::list_names() {
        Ok(names) => !names.is_empty(),
        Err(_) => true,
    }
}

/// Walks the `doctor` flag prefix the way Go's `flag.FlagSet` does and reports
/// whether a surviving `-vault-agent` still needs the shipped handshake.
///
/// Go stops parsing at `-h`/`-help`, at an undefined flag and at a missing flag
/// value, and all three print usage and exit 2 before any handshake happens. A
/// value flag consumes the following argument even when that argument looks
/// like a flag, so `doctor --vault-agent --force-release --help` never reaches
/// the handshake and must stay native.
fn vault_agent_with_profiles_requires_go(
    args: &[OsString],
    profiles_require_go: impl FnOnce() -> bool,
) -> bool {
    let normalized = crate::normalize_flags(args);
    let mut index = 0;
    let mut saw_vault_agent = false;
    while index < normalized.len() {
        let argument = normalized[index].to_string_lossy();
        if argument == "--" || argument == "-" || !argument.starts_with('-') {
            break;
        }
        let flag = argument.trim_start_matches('-');
        let (name, value) = flag
            .split_once('=')
            .map_or((flag, None), |(name, value)| (name, Some(value)));
        if !["json", "fix", "force-release", "vault-agent", "h", "help"].contains(&name) {
            break;
        }
        if matches!(name, "h" | "help") {
            return false;
        }
        if name == "vault-agent" {
            saw_vault_agent = true;
            if value.is_none() {
                index += 1;
                if index == normalized.len() {
                    return false;
                }
            }
        }
        index += 1;
    }
    saw_vault_agent && profiles_require_go()
}

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
                    // Go's flag package prints the parse error and then the
                    // whole flag set usage for a missing value argument.
                    let _ = writeln!(stderr, "flag needs an argument: -vault-agent");
                    let _ = write!(stderr, "{DOCTOR_USAGE}");
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
            OsString::from("--vault-agent"),
            OsString::from("agent"),
        ];
        let parsed = parse_args(&args, &mut Vec::new()).expect("valid flags");
        assert!(!parsed.fix);
        assert_eq!(parsed.vault_agent, "agent");
    }

    #[test]
    fn vault_agent_fallback_requires_reliably_listed_profiles() {
        // `requires_go_fallback` receives the arguments after the command
        // name, so the case does not repeat `doctor`.
        let args = |rest: &[&str]| -> Vec<OsString> { rest.iter().map(OsString::from).collect() };

        // A surviving `-vault-agent` consults the machine's profiles; the
        // precondition is injected here so the case never reads the real home.
        assert!(!requires_go_fallback_with(
            &args(&["--vault-agent", "agent"]),
            || false
        ));
        assert!(requires_go_fallback_with(
            &args(&["--vault-agent", "agent"]),
            || true
        ));
        assert!(requires_go_fallback_with(
            &args(&["--vault-agent=agent"]),
            || true
        ));
        assert!(!requires_go_fallback_with(&args(&["--json"]), || true));

        // Go's flag package stops before the handshake (usage, exit 2) and
        // consumes the next argument as the `-vault-agent` value, so a later
        // flag must never reach the profile lookup.
        assert!(!requires_go_fallback_with(
            &args(&["--vault-agent", "--force-release", "--help"]),
            || true
        ));
        assert!(!requires_go_fallback_with(
            &args(&["--vault-agent=agent", "--help"]),
            || true
        ));
        assert!(!requires_go_fallback_with(
            &args(&["--vault-agent"]),
            || true
        ));
        assert!(!requires_go_fallback_with(
            &args(&["--unknown", "--vault-agent", "agent"]),
            || true
        ));

        // The lifecycle flags keep the shipped installer semantics regardless
        // of the profile precondition.
        assert!(requires_go_fallback_with(
            &args(&["--force-release"]),
            || false
        ));
        assert!(requires_go_fallback_with(&args(&["--fix"]), || false));
    }

    #[test]
    fn help_lists_the_go_owned_force_release_flag() {
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
