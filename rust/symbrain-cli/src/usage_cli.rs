use std::ffi::OsString;
use std::io::Write;

use symbrain_core::exit;
use symbrain_core::output::{self, OutputFormat};
use symbrain_usage::{Report, Service, UsageMeter};

const HELP: &str = "symbrain usage — AI subscription/token usage per provider\n\nUsage:\n  symbrain usage\n\nThe global --output table|json flag (or --json) selects the output format.\n\nProviders: Claude, Codex, Copilot, Cursor, Kimi, Moonshot, Nous Portal,\nOpenCode, OpenRouter, Antigravity. Credential resolution: an explicit env\nvar per provider, whose value may be a symvault://<path> URI resolved\nthrough the secret store; Claude, Codex, and Copilot accept narrowly proven\ndefault credential-file shapes read-only when the env var is unset. See each provider's\ndoc comment in internal/usage for the macOS-Keychain / local-database\nstrategies not ported from the Swift original.\n";

/// Reports whether `symbrain usage` has to stay on the Go implementation.
///
/// The native port reproduces each proven provider's credential state machine,
/// request, snapshot, and error behavior. Several supported providers can run
/// together. Native sources include direct credentials; the canonical default
/// Copilot, Kimi CLI, Nous, Codex, and Claude files; Moonshot's supported `ai`
/// and `cn` regions; constrained public HTTPS base overrides; and a canonical
/// `OpenCode` workspace id. File sources with case aliases, duplicate or
/// malformed fields, ambiguous provider selection, unsupported metadata,
/// secret references, non-ASCII Kimi device ids, or other unproven shapes stay
/// on Go. `CODEX_HOME`, `HERMES_HOME`, and `KIMI_CODE_HOME` use the same per-file
/// eligibility checks as their default paths; differing Windows home roots,
/// unsupported URL/workspace forms, Claude Keychain-only credentials, and a
/// running Antigravity provider also keep the report on Go. Source-bound Go
/// oracles use synthetic credentials and canned transport; the user-invoked
/// live report remains unchanged.
pub(crate) fn requires_go_fallback(args: &[OsString]) -> bool {
    if args.len() == 1 && matches!(args[0].to_str(), Some("-h" | "--help")) {
        return false;
    }
    symbrain_usage::needs_go_fallback()
}

pub fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let args = normalize_flags(args);
    if let Some(arg) = args.iter().find(|arg| {
        let value = arg.to_string_lossy();
        value == "-h" || value == "-help" || value == "--help"
    }) {
        if args.len() == 1 {
            let _ = write!(stderr, "{HELP}");
            return exit::USAGE;
        }
        let _ = writeln!(
            stderr,
            "symbrain usage: unexpected argument {}",
            debug_arg(arg)
        );
        return exit::USAGE;
    }
    if let Some(arg) = args
        .iter()
        .find(|arg| arg.to_string_lossy().starts_with('-'))
    {
        let name = arg.to_string_lossy();
        let name = name.trim_start_matches('-').split('=').next().unwrap_or("");
        let _ = writeln!(stderr, "flag provided but not defined: -{name}");
        let _ = write!(stderr, "{HELP}");
        return exit::USAGE;
    }
    if let Some(arg) = args.first() {
        let _ = writeln!(
            stderr,
            "symbrain usage: unexpected argument {}",
            debug_arg(arg)
        );
        return exit::USAGE;
    }

    let report = Service::new().report();
    let result = output::render(stdout, format, &report, |writer| {
        render_report_table(writer, &report)
    });
    if result.is_err() {
        let _ = writeln!(stderr, "symbrain usage: format output");
        return exit::GENERIC;
    }
    exit::OK
}

#[allow(clippy::unnecessary_debug_formatting)]
fn debug_arg(arg: &OsString) -> String {
    format!("{arg:?}")
}

fn render_report_table(writer: &mut dyn Write, report: &Report) -> std::io::Result<()> {
    for provider in &report.providers {
        write!(writer, "{}\t{}\t", provider.id, provider.display_name)?;
        if !provider.configured {
            writeln!(writer, "not configured\t{}", provider.auth_status.detail)?;
        } else if let Some(error) = &provider.error {
            writeln!(writer, "error\t{error}")?;
        } else if let Some(snapshot) = &provider.snapshot {
            writeln!(writer, "ok\tsource={}", snapshot.source)?;
            for meter in &snapshot.meters {
                writeln!(
                    writer,
                    "  {}\t{}\t{}",
                    meter.label,
                    meter_value(meter),
                    meter.unit
                )?;
            }
        } else {
            writeln!(writer, "unknown")?;
        }
    }
    Ok(())
}

fn meter_value(meter: &UsageMeter) -> String {
    let used = meter.used.as_deref().unwrap_or("?");
    meter
        .limit
        .as_deref()
        .map_or_else(|| used.to_owned(), |limit| format!("{used}/{limit}"))
}

fn normalize_flags(args: &[OsString]) -> Vec<OsString> {
    args.iter()
        .map(|arg| {
            let value = arg.to_string_lossy();
            if value.starts_with("--") && value.len() > 2 {
                OsString::from(format!("-{}", &value[2..]))
            } else {
                arg.clone()
            }
        })
        .collect()
}

// Keep table rendering testable without making the public usage report depend
// on CLI concerns.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_is_native_without_resolving_credentials() {
        for flag in ["-h", "--help"] {
            let args = [OsString::from(flag)];
            assert!(!requires_go_fallback(&args));
            let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
            assert_eq!(
                run(&args, &mut stdout, &mut stderr, OutputFormat::Table),
                exit::USAGE
            );
            assert!(stdout.is_empty());
            assert_eq!(stderr, HELP.as_bytes());
        }
    }

    #[test]
    fn table_matches_go_shape() {
        let report = Report {
            schema_version: 1,
            providers: vec![symbrain_usage::ProviderUsage {
                id: "x".into(),
                display_name: "X".into(),
                configured: true,
                auth_status: symbrain_usage::AuthStatus::default(),
                snapshot: Some(symbrain_usage::UsageSnapshot {
                    provider_id: "x".into(),
                    meters: vec![UsageMeter {
                        label: "quota".into(),
                        used: Some("1".into()),
                        limit: Some("2".into()),
                        unit: "%".into(),
                        resets_at: None,
                    }],
                    balance: None,
                    currency: None,
                    fetched_at: chrono::DateTime::UNIX_EPOCH,
                    source: "api".into(),
                }),
                error: None,
            }],
        };
        let mut out = Vec::new();
        render_report_table(&mut out, &report).expect("render");
        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            "x\tX\tok\tsource=api\n  quota\t1/2\t%\n"
        );
    }
}
