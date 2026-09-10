use std::ffi::OsString;
use std::io::Write;

use chrono::{DateTime, Local};
use symbrain_audit::Entry;
use symbrain_core::exit;
use symbrain_core::output::OutputFormat;

use crate::normalize_flags;

pub(crate) fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let normalized = normalize_flags(args);
    let Some((subcommand, rest)) = parse_audit_command(&normalized, stderr) else {
        return exit::USAGE;
    };
    match subcommand.as_str() {
        "tail" => run_tail(rest, stdout, stderr, format),
        unknown => {
            let _ = writeln!(stderr, "symbrain audit: unknown subcommand {unknown:?}");
            exit::USAGE
        }
    }
}

fn parse_audit_command<'a>(
    args: &'a [OsString],
    stderr: &mut dyn Write,
) -> Option<(String, &'a [OsString])> {
    if args.is_empty() {
        let _ = writeln!(stderr, "symbrain audit: subcommand required (tail)");
        return None;
    }
    let first = args[0].to_string_lossy();
    if first == "-h" || first == "-help" {
        let _ = writeln!(stderr, "Usage of audit:");
        return None;
    }
    if first.starts_with('-') {
        let name = first
            .trim_start_matches('-')
            .split_once('=')
            .map_or(first.trim_start_matches('-'), |(name, _)| name);
        let _ = writeln!(stderr, "flag provided but not defined: -{name}");
        let _ = writeln!(stderr, "Usage of audit:");
        return None;
    }
    Some((first.into_owned(), &args[1..]))
}

fn run_tail(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let Ok(options) = parse_tail_options(args, stderr) else {
        return exit::USAGE;
    };
    let limit = usize::try_from(options.limit).unwrap_or(0);
    let entries = match symbrain_audit::tail_entries(&options.profile, limit) {
        Ok(entries) => entries,
        Err(error) => {
            let suffix = if format == OutputFormat::Json {
                " --json"
            } else {
                ""
            };
            let _ = writeln!(stderr, "symbrain audit tail{suffix}: {error}");
            return exit::GENERIC;
        }
    };

    let result = if format == OutputFormat::Json {
        if entries.is_empty() {
            writeln!(stdout, "null")
        } else {
            serde_json::to_writer(&mut *stdout, &entries)
                .map_err(std::io::Error::other)
                .and_then(|()| writeln!(stdout))
        }
    } else {
        entries
            .iter()
            .try_for_each(|entry| print_entry(stdout, entry))
    };
    if result.is_err() {
        let suffix = if format == OutputFormat::Json {
            " --json: encode"
        } else {
            ""
        };
        let _ = writeln!(stderr, "symbrain audit tail{suffix}: format output");
        exit::GENERIC
    } else {
        exit::OK
    }
}

#[derive(Debug, PartialEq, Eq)]
struct TailOptions {
    profile: String,
    limit: i64,
}

fn parse_tail_options(args: &[OsString], stderr: &mut dyn Write) -> Result<TailOptions, ()> {
    let mut profile = String::new();
    let mut limit = 20_i64;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        if arg == "--" || !arg.starts_with('-') || arg == "-" {
            break;
        }
        let flag = arg.trim_start_matches('-');
        if flag == "h" || flag == "help" {
            print_tail_usage(stderr);
            return Err(());
        }
        if flag == "json" {
            index += 1;
            continue;
        }
        let (name, inline) = flag
            .split_once('=')
            .map_or((flag, None), |(name, value)| (name, Some(value)));
        if name != "profile" && name != "n" {
            let _ = writeln!(stderr, "flag provided but not defined: -{name}");
            print_tail_usage(stderr);
            return Err(());
        }
        let value = if let Some(value) = inline {
            value.to_string()
        } else {
            index += 1;
            let Some(value) = args.get(index) else {
                let _ = writeln!(stderr, "flag needs an argument: -{name}");
                print_tail_usage(stderr);
                return Err(());
            };
            value.to_string_lossy().into_owned()
        };
        if name == "profile" {
            profile = value;
        } else {
            let Ok(parsed) = value.parse::<i64>() else {
                let _ = writeln!(stderr, "invalid value {value:?} for flag -n: parse error");
                print_tail_usage(stderr);
                return Err(());
            };
            limit = parsed;
        }
        index += 1;
    }
    Ok(TailOptions { profile, limit })
}

fn print_tail_usage(stderr: &mut dyn Write) {
    let _ = write!(
        stderr,
        "Usage of audit tail:\n  -json\n    \temit machine-readable JSON\n  -n int\n    \tnumber of entries to show (default 20)\n  -profile string\n    \tfilter by profile name\n"
    );
}

fn print_entry(stdout: &mut dyn Write, entry: &Entry) -> std::io::Result<()> {
    let local = DateTime::parse_from_rfc3339(&entry.timestamp).map_or_else(
        |_| "0001-01-01 00:00:00".to_string(),
        |timestamp| {
            timestamp
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        },
    );
    let extra = if entry.arg_keys.is_empty() {
        String::new()
    } else {
        format!(" keys={}", entry.arg_keys)
    };
    writeln!(
        stdout,
        "{local}  {:<10}  {:<8}  {:<25}  {}ms  {}{}",
        entry.profile, entry.server, entry.tool, entry.duration_ms, entry.status, extra
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tail_flags_and_ignores_positionals_after_first() {
        let args = ["-profile=work", "-n", "-2", "ignored", "-n", "5"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();
        let options = parse_tail_options(&args, &mut Vec::new()).expect("parse");
        assert_eq!(
            options,
            TailOptions {
                profile: "work".to_string(),
                limit: -2
            }
        );
    }

    #[test]
    fn formats_human_entry_like_go() {
        let mut output = Vec::new();
        print_entry(
            &mut output,
            &Entry {
                timestamp: "2026-01-01T00:00:00Z".to_string(),
                profile: "p".to_string(),
                server: "memory".to_string(),
                tool: "search".to_string(),
                duration_ms: 42,
                status: "ok".to_string(),
                arg_keys: "query".to_string(),
                ..Entry::default()
            },
        )
        .expect("format");
        let output = String::from_utf8(output).expect("utf8");
        assert!(output.contains("memory"));
        assert!(output.contains("42ms  ok keys=query"));
    }
}
