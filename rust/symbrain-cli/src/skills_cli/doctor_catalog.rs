//! Opt-in catalog section of `symbrain skills doctor`.
//!
//! `doctor`'s default output is frozen against the Go oracle, so the catalog
//! diagnostics sit behind `--catalog` (and `--context-window <tokens>`, which
//! implies it). Those two flags are lifted out of the argument list before the
//! Go-exact flag parser runs; every other argument, including its error
//! diagnostics, is untouched.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::Write;

use symbrain_core::exit;
use symbrain_skills::catalog::{CatalogReport, TargetCatalog};

/// Catalog flags lifted out of the `doctor` arguments.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Opts {
    /// Print the catalog section.
    pub(super) catalog: bool,
    /// Explicit context window in tokens.
    pub(super) context_window: Option<usize>,
}

const USAGE: &str = "Usage: symbrain skills doctor [--catalog] [--context-window <tokens>]\n";

/// Splits `args` into the catalog flags and the arguments left for the Go flag set.
///
/// # Errors
///
/// The exit code after a diagnostic for an invalid `--context-window` value.
pub(super) fn extract(
    args: &[OsString],
    stderr: &mut dyn Write,
) -> Result<(Vec<OsString>, Opts), u8> {
    let mut rest = Vec::new();
    let mut opts = Opts::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let Some(text) = arg.to_str() else {
            rest.push(arg.clone());
            continue;
        };
        if text == "--" {
            rest.push(arg.clone());
            rest.extend(iter.by_ref().cloned());
            break;
        }
        let body = text.strip_prefix("--").or_else(|| text.strip_prefix('-'));
        let (name, inline) = match body.and_then(|body| body.split_once('=')) {
            Some((name, value)) => (name, Some(value)),
            None => (body.unwrap_or_default(), None),
        };
        match name {
            "catalog" => match inline {
                None | Some("true") => opts.catalog = true,
                Some("false") => opts.catalog = false,
                Some(other) => {
                    return Err(fail(
                        stderr,
                        &format!("invalid boolean value {other:?} for --catalog"),
                    ));
                }
            },
            "context-window" => {
                let value = match inline {
                    Some(value) => Some(value.to_owned()),
                    None => iter
                        .next()
                        .and_then(|value| value.to_str())
                        .map(str::to_owned),
                };
                let Some(value) = value else {
                    return Err(fail(stderr, "flag needs an argument: --context-window"));
                };
                match value.parse::<usize>() {
                    Ok(tokens) if tokens > 0 => {
                        opts.context_window = Some(tokens);
                        opts.catalog = true;
                    }
                    _ => {
                        let message = format!(
                            "invalid --context-window {value:?}: want a positive token count"
                        );
                        return Err(fail(stderr, &message));
                    }
                }
            }
            _ => rest.push(arg.clone()),
        }
    }
    Ok((rest, opts))
}

fn fail(stderr: &mut dyn Write, message: &str) -> u8 {
    let _ = writeln!(stderr, "symbrain skills doctor: {message}\n");
    let _ = write!(stderr, "{USAGE}");
    exit::USAGE
}

/// Renders the catalog section for the table output, one block per target.
pub(super) fn table(report: &CatalogReport) -> String {
    let mut out = format!(
        "catalog     context window {} tokens ({}); lookalike threshold {}/1000 over >= {} words\n",
        report.context_window_tokens,
        report.context_window_source,
        report.lookalike_threshold_permille,
        report.lookalike_min_words,
    );
    for target in &report.targets {
        out.push_str(&target_block(target));
    }
    out
}

fn target_block(target: &TargetCatalog) -> String {
    let budget = match (target.listing_budget_chars, target.over_by) {
        (Some(limit), Some(0)) => format!("budget {limit} chars, under"),
        (Some(limit), Some(over)) => format!("budget {limit} chars, OVER by {over}"),
        _ => "budget not profiled".to_owned(),
    };
    let mut out = format!(
        "{:<11} {} skills, {} description chars, {budget}\n",
        target.target, target.skill_count, target.description_chars
    );
    for duplicate in &target.duplicates {
        let _ = writeln!(
            out,
            "  duplicate   {} (harness uses: {})",
            duplicate.name, duplicate.harness_uses
        );
        for copy in &duplicate.copies {
            let state = if copy.managed { "managed" } else { "unmanaged" };
            let _ = writeln!(out, "    {:<8} {state:<9} {}", copy.scope, copy.path);
        }
    }
    for pair in &target.lookalikes {
        let _ = writeln!(
            out,
            "  lookalike   {} ~ {} ({}/1000)",
            pair.a, pair.b, pair.similarity_permille
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn catalog_flags_are_lifted_and_the_rest_is_untouched() {
        for (input, want_rest, want) in [
            (&["--json"][..], &["--json"][..], Opts::default()),
            (
                &["--catalog"],
                &[],
                Opts {
                    catalog: true,
                    context_window: None,
                },
            ),
            (
                &["-catalog=true", "--bogus"],
                &["--bogus"],
                Opts {
                    catalog: true,
                    context_window: None,
                },
            ),
            (&["--catalog=false"], &[], Opts::default()),
            (
                &["--context-window", "1000000"],
                &[],
                Opts {
                    catalog: true,
                    context_window: Some(1_000_000),
                },
            ),
            (
                &["--context-window=8000", "--target", "x"],
                &["--target", "x"],
                Opts {
                    catalog: true,
                    context_window: Some(8000),
                },
            ),
            // After `--` nothing is interpreted.
            (&["--", "--catalog"], &["--", "--catalog"], Opts::default()),
        ] {
            let mut stderr = Vec::new();
            let (rest, opts) = extract(&args(input), &mut stderr).unwrap();
            assert_eq!(rest, args(want_rest), "{input:?}");
            assert_eq!(opts, want, "{input:?}");
            assert!(stderr.is_empty());
        }
    }

    #[test]
    fn invalid_context_windows_are_usage_errors() {
        for input in [
            &["--context-window", "0"][..],
            &["--context-window=abc"],
            &["--context-window"],
            &["--catalog=maybe"],
        ] {
            let mut stderr = Vec::new();
            assert_eq!(
                extract(&args(input), &mut stderr).unwrap_err(),
                exit::USAGE,
                "{input:?}"
            );
            assert!(
                String::from_utf8(stderr)
                    .unwrap()
                    .contains("Usage: symbrain skills doctor")
            );
        }
    }
}
