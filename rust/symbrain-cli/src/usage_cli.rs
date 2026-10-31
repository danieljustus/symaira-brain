use std::ffi::OsString;
use std::io::Write;

use symbrain_core::config::format_go_quoted;
use symbrain_core::exit;
use symbrain_core::output::{self, OutputFormat};
use symbrain_usage::{ProviderSelection, Report, Service, UsageMeter};

const HELP: &str = "symbrain usage — AI subscription/token usage per provider\n\nUsage:\n  symbrain usage [--providers ID,ID]\n\n--providers selects only those providers before credential discovery.\nDefault IDs: claude,codex,copilot,cursor,kimi,moonshot,nous,opencode,openrouter,antigravity.\nOpt-in IDs: zai (ZAI_API_KEY), devin (DEVIN_API_KEY, Codeium-compatible),\ngrok (GROK_ACCESS_TOKEN or GROK_HOME/auth.json), ollama (local signing key only).\n\nThe global --output table|json flag (or --json) selects the output format.\n\nProviders: Claude, Codex, Copilot, Cursor, Kimi, Moonshot, Nous Portal,\nOpenCode, OpenRouter, Antigravity. Credential resolution: an explicit env\nvar per provider, whose value may be a symvault://<path> URI resolved\nthrough the secret store; providers with a native CLI credential file\nfall back to it read-only when the env var is unset. See each provider's\ndoc comment in internal/usage for the macOS-Keychain / local-database\nstrategies not ported from the Swift original.\n";

/// A Usage invocation classified before any credential admission or reads.
pub(crate) enum Invocation {
    Report(ProviderSelection),
    SelectionError(String),
    MissingProviders,
    Help,
    BadSyntax(Vec<u8>),
    UndefinedFlag(Vec<u8>),
    Unexpected(OsString),
}

pub(crate) fn classify(args: &[OsString]) -> Invocation {
    let args = crate::normalize_flags(args);
    let selection = ProviderSelection::default();
    let mut selector = None;
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        let bytes = arg.as_encoded_bytes();
        if bytes == b"--" {
            return args.get(index + 1).map_or_else(
                || selected_invocation(selector, selection),
                |arg| Invocation::Unexpected(arg.clone()),
            );
        }
        if bytes.len() < 2 || !bytes.starts_with(b"-") {
            return Invocation::Unexpected(arg.clone());
        }
        let name = &bytes[if bytes[1] == b'-' { 2 } else { 1 }..];
        if name.is_empty() || matches!(name[0], b'-' | b'=') {
            return Invocation::BadSyntax(bytes.to_vec());
        }
        let mut parts = name.splitn(2, |byte| *byte == b'=');
        let name = parts.next().unwrap_or_default();
        if matches!(name, b"h" | b"help") {
            return Invocation::Help;
        }
        if name != b"providers" {
            return Invocation::UndefinedFlag(name.to_vec());
        }
        let value = if let Some(value) = parts.next() {
            value
        } else {
            index += 1;
            let Some(value) = args.get(index) else {
                return Invocation::MissingProviders;
            };
            value.as_encoded_bytes()
        };
        selector = Some(String::from_utf8_lossy(value).into_owned());
        index += 1;
    }
    // Match FlagSet: the final repeated flag wins, then validate all IDs.
    selected_invocation(selector, selection)
}

fn selected_invocation(selector: Option<String>, default: ProviderSelection) -> Invocation {
    match selector {
        None => Invocation::Report(default),
        Some(value) => match ProviderSelection::parse(&value) {
            Ok(selection) => Invocation::Report(selection),
            Err(error) => Invocation::SelectionError(error),
        },
    }
}

// Free-function entry points keep `lib.rs` byte-identical to the inherited
// global-output owner pinned by the Skills argv oracle (novel.py).
pub(crate) fn requires_go_fallback(args: &[OsString]) -> bool {
    classify(args).requires_go_fallback()
}

pub fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    classify(args).run(stdout, stderr, format)
}

impl Invocation {
    pub(crate) fn requires_go_fallback(&self) -> bool {
        matches!(self, Self::Report(selection) if symbrain_usage::needs_go_fallback_for_selection(selection))
    }

    pub(crate) fn run(
        self,
        stdout: &mut dyn Write,
        stderr: &mut dyn Write,
        format: OutputFormat,
    ) -> u8 {
        match self {
            Self::Report(selection) => run_report(&selection, stdout, stderr, format),
            Self::SelectionError(error) => {
                let _ = writeln!(stderr, "symbrain usage: {error}");
                exit::USAGE
            }
            Self::MissingProviders => {
                flag_error(stderr, b"flag needs an argument: -", b"providers")
            }
            Self::Help => {
                let _ = stderr.write_all(HELP.as_bytes());
                exit::USAGE
            }
            Self::BadSyntax(value) => flag_error(stderr, b"bad flag syntax: ", &value),
            Self::UndefinedFlag(value) => {
                flag_error(stderr, b"flag provided but not defined: -", &value)
            }
            Self::Unexpected(arg) => {
                let _ = writeln!(
                    stderr,
                    "symbrain usage: unexpected argument {}",
                    format_go_quoted(&arg)
                );
                exit::USAGE
            }
        }
    }
}

fn run_report(
    selection: &ProviderSelection,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let report = Service::with_selection(selection).report();
    let result = output::render(stdout, format, &report, |writer| {
        render_report_table(writer, &report)
    });
    if result.is_err() {
        let _ = writeln!(stderr, "symbrain usage: format output");
        return exit::GENERIC;
    }
    exit::OK
}

fn flag_error(stderr: &mut dyn Write, prefix: &[u8], value: &[u8]) -> u8 {
    let _ = stderr.write_all(prefix);
    let _ = stderr.write_all(value);
    let _ = stderr.write_all(b"\n");
    let _ = stderr.write_all(HELP.as_bytes());
    exit::USAGE
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
        if let Some(h) = &provider.history {
            writeln!(
                writer,
                "  local history\t{}\tscope={} attribution={}",
                h.status, h.scope, h.attribution
            )?;
            for period in &h.periods {
                for model in &period.models {
                    writeln!(
                        writer,
                        "  {}\t{}\t{} tokens",
                        period.key,
                        model.model,
                        model.tokens.as_deref().unwrap_or("?")
                    )?;
                }
            }
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

// Keep table rendering testable without making the public usage report depend
// on CLI concerns.
#[cfg(test)]
#[path = "usage_fetch_output_tests.rs"]
mod fetch_output_tests;
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_is_native_without_resolving_credentials() {
        for flag in ["-h", "--help"] {
            let args = [OsString::from(flag)];
            let invocation = classify(&args);
            assert!(!invocation.requires_go_fallback());
            let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
            assert_eq!(
                invocation.run(&mut stdout, &mut stderr, OutputFormat::Table),
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
                history: None,
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
