use std::ffi::OsString;
use std::io::Write;

use symbrain_core::exit;
use symbrain_core::output::{self, OutputFormat};
use symbrain_usage::{Report, Service, UsageMeter};

const HELP: &str = "symbrain usage — AI subscription/token usage per provider\n\nUsage:\n  symbrain usage\n\nThe global --output table|json flag (or --json) selects the output format.\n\nProviders: Claude, Codex, Copilot, Cursor, Kimi, Moonshot, Nous Portal,\nOpenCode, OpenRouter, Antigravity. Credentials are read-only and their\nvalues are never included in output, errors, or audit records.\n";

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
