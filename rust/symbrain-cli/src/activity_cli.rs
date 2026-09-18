//! Native `symbrain activity` CLI implementation.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use crate::go_json;
use chrono::{DateTime, Utc};
use symbrain_activity::{fence_summary, profile_allows_activity};
use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_memory::ActivitySearch;
use symbrain_memory::Store;
use symbrain_policy::load as load_profile;

const ACTIVITY_USAGE: &str = "symbrain activity \u{2014} bounded, profile-gated activity reads

Usage:
  symbrain activity <search|get|status> [flags]

Every command requires --profile, an explicit bounded response budget, and (for search) an explicit RFC3339 window and result limit.
";

fn resolve_db_path() -> PathBuf {
    if let Some(path) = std::env::var_os("SYMBRAIN_MEMORY_DB_PATH") {
        return PathBuf::from(path);
    }
    if let Some(data) = symbrain_core::xdg::data_dir() {
        let preferred = data.join("memory").join("memory.db");
        if preferred.exists() {
            return preferred;
        }
    }
    symbrain_core::xdg::data_dir().map_or_else(
        || PathBuf::from(".memory.db"),
        |d| d.join("memory").join("memory.db"),
    )
}

/// Reports whether `symbrain activity` has to stay on the Go implementation.
///
/// Native today: the usage/dispatch text and the policy message for a profile
/// that is absent, unloadable or does not expose the activity read tools.
/// Those bytes are pinned by differential cases.
///
/// Still on Go: `search`, `get` and `status` themselves - the shipped
/// implementations carry a bounded response budget, token-fenced summaries,
/// TTL fields and a result page the native path does not reproduce yet.
pub(crate) fn requires_go_fallback(args: &[OsString]) -> bool {
    let Some(verb) = args.first().map(|arg| arg.to_string_lossy().into_owned()) else {
        return false;
    };
    if matches!(verb.as_str(), "-h" | "--help" | "help") {
        return false;
    }
    // An unusable profile is answered natively; a usable one falls through to
    // the subcommand, which stays on Go.
    let profile_name = extract_flag(args, "--profile").or_else(|| extract_flag(args, "-profile"));
    let Some(name) = profile_name else {
        return false;
    };
    let allowed = match load_profile(&name) {
        Ok(profile) => profile_allows_activity(&profile),
        Err(_) => return false,
    };
    if !allowed {
        return false;
    }
    // A granting profile: `status` and `get` run natively, `search` still
    // needs the shipped budget/window/page semantics.
    match verb.as_str() {
        "status" => false,
        // The shipped flag set stops at the first bare argument, so a flag
        // after the identifier is a usage error there; only the documented
        // `get <flags> <id>` order is reproducible natively.
        "get" => !get_argument_order_is_shipped(&args[1..]),
        _ => true,
    }
}

/// Reports whether `get` receives its flags before the single identifier.
fn get_argument_order_is_shipped(args: &[OsString]) -> bool {
    let mut positionals = 0;
    let mut seen_positional = false;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg.as_ref(), None), |(name, value)| (name, Some(value)));
        let flag = [
            "--profile",
            "-profile",
            "--max-tokens",
            "-max-tokens",
            "--db",
            "-db",
        ];
        if flag.contains(&name) {
            if seen_positional || inline.is_none() {
                return false;
            }
        } else if arg.starts_with('-') {
            return false;
        } else {
            positionals += 1;
            seen_positional = true;
        }
        index += 1;
    }
    positionals == 1
}

/// Runs `symbrain activity`.
pub fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    if args.is_empty() {
        let _ = write!(stdout, "{ACTIVITY_USAGE}");
        return exit::USAGE;
    }

    let verb = args[0].to_string_lossy();
    if matches!(verb.as_ref(), "-h" | "--help" | "help") {
        let _ = write!(stdout, "{ACTIVITY_USAGE}");
        return exit::OK;
    }

    // Check profile
    let profile_name = extract_flag(args, "--profile").or_else(|| extract_flag(args, "-profile"));
    let Some(pname) = profile_name else {
        let _ = writeln!(
            stderr,
            "symbrain activity: --profile is required and must explicitly expose activity read tools"
        );
        return exit::USAGE;
    };

    // The shipped command answers an unusable profile with its policy
    // message, never with the loader's diagnostic.
    let Ok(profile) = load_profile(&pname) else {
        let _ = writeln!(
            stderr,
            "symbrain activity: --profile is required and must explicitly expose activity read tools"
        );
        return exit::USAGE;
    };

    if !profile_allows_activity(&profile) {
        let _ = writeln!(
            stderr,
            "symbrain activity: --profile is required and must explicitly expose activity read tools"
        );
        return exit::USAGE;
    }

    let rest = &args[1..];
    match verb.as_ref() {
        "search" => run_search(rest, stdout, stderr, format),
        "get" => run_get(rest, stdout, stderr, format),
        "status" => run_status(rest, stdout, stderr, format),
        _ => {
            let _ = writeln!(stderr, "symbrain activity: unknown subcommand {verb:?}\n");
            let _ = write!(stderr, "{ACTIVITY_USAGE}");
            exit::USAGE
        }
    }
}

fn extract_flag(args: &[OsString], flag: &str) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == flag {
            if i + 1 < args.len() {
                return Some(args[i + 1].to_string_lossy().into_owned());
            }
        } else if let Some(v) = arg.strip_prefix(&format!("{flag}=")) {
            return Some(v.to_string());
        }
        i += 1;
    }
    None
}

#[allow(clippy::too_many_lines)]
/// Opens the activity store, reporting the shipped per-subcommand message.
fn open_store(stderr: &mut dyn Write, subcommand: &str) -> Result<Store, u8> {
    let db_path = resolve_db_path();
    match Store::open(&db_path) {
        Ok(store) => Ok(store),
        Err(err) => {
            let _ = writeln!(
                stderr,
                "symbrain activity {subcommand}: open database: {err}"
            );
            Err(exit::GENERIC)
        }
    }
}

fn run_search(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let query = positional_argument(
        args,
        &[
            "--profile",
            "-profile",
            "--from",
            "-from",
            "--to",
            "-to",
            "--limit",
            "-limit",
            "--max-tokens",
            "-max-tokens",
            "--db",
            "-db",
        ],
    )
    .unwrap_or_default();
    let from: Option<DateTime<Utc>> =
        flag_value(args, &["--from", "-from"]).and_then(|value| value.parse().ok());
    let to: Option<DateTime<Utc>> =
        flag_value(args, &["--to", "-to"]).and_then(|value| value.parse().ok());
    let limit = flag_value(args, &["--limit", "-limit"])
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(10);
    let max_tokens = flag_value(args, &["--max-tokens", "-max-tokens"])
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1000);

    let store = match open_store(stderr, "search") {
        Ok(store) => store,
        Err(code) => return code,
    };

    // `search` stays on Go (see the gate); this path only needs to compile
    // and behave sanely for callers that reach it directly.
    let from_dt = from.unwrap_or_else(Utc::now);
    let to_dt = to.unwrap_or_else(Utc::now);
    let mem_search = ActivitySearch {
        query,
        source: String::new(),
        from: from_dt,
        to: to_dt,
        limit,
        max_tokens,
        include_episodes: false,
    };

    let page = match store.activity_search(&mem_search) {
        Ok(p) => p,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain activity search: {err}");
            return exit::GENERIC;
        }
    };

    match format {
        OutputFormat::Json => {
            let _ = writeln!(
                stdout,
                "{}",
                serde_json::to_string_pretty(&page).unwrap_or_default()
            );
        }
        OutputFormat::Table => {
            if page.results.is_empty() {
                let _ = writeln!(stdout, "No activity matching query.");
            } else {
                for item in &page.results {
                    let fenced = fence_summary(&item.summary, max_tokens);
                    let _ = writeln!(
                        stdout,
                        "{}\t{}\n{}\n",
                        item.id,
                        item.started_at.format("%Y-%m-%d %H:%M"),
                        fenced
                    );
                }
            }
        }
    }

    exit::OK
}

fn run_status(
    _args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let store = match open_store(stderr, "status") {
        Ok(store) => store,
        Err(code) => return code,
    };
    let status = match store.activity_status() {
        Ok(status) => status,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain activity status: status: {err}");
            return exit::GENERIC;
        }
    };

    match format {
        OutputFormat::Json => {
            let _ = writeln!(stdout, "{}", go_json(&status));
        }
        OutputFormat::Table => {
            let _ = writeln!(
                stdout,
                "segments={}\tepisodes={}",
                status.active_segments, status.active_episodes
            );
        }
    }

    exit::OK
}

fn run_get(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let flags = [
        "--profile",
        "-profile",
        "--max-tokens",
        "-max-tokens",
        "--db",
        "-db",
    ];
    let max_tokens = flag_value(args, &["--max-tokens", "-max-tokens"])
        .and_then(|value| value.parse::<usize>().ok());
    let Some(id) = positional_argument(args, &flags) else {
        let _ = writeln!(
            stderr,
            "usage: symbrain activity get <id> --profile <name> --max-tokens <N> [--db <path>]"
        );
        return exit::USAGE;
    };
    let store = match open_store(stderr, "get") {
        Ok(store) => store,
        Err(code) => return code,
    };
    let item = match store.activity_get(&id) {
        Ok(item) => item,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain activity get: get: {err}");
            return exit::GENERIC;
        }
    };
    let Some(mut item) = item else {
        let _ = writeln!(stderr, "symbrain activity get: activity not found: {id}");
        return exit::USAGE;
    };

    // The shipped command fences the summary and reports its token count.
    let summary = fence_summary(&item.summary, max_tokens.unwrap_or(0));
    item.tokens = if summary.is_empty() {
        0
    } else {
        summary.chars().count() / 4 + 1
    };
    item.summary = summary;

    match format {
        OutputFormat::Json => {
            let _ = writeln!(stdout, "{}", go_json(&item));
        }
        OutputFormat::Table => {
            let _ = writeln!(
                stdout,
                "{}\t{}\t{}",
                item.started_at.format("%Y-%m-%dT%H:%M:%SZ"),
                item.kind,
                item.summary
            );
        }
    }

    exit::OK
}

/// Reads a flag's value in `--flag value` or `--flag=value` form.
fn flag_value(args: &[OsString], names: &[&str]) -> Option<String> {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg.as_ref(), None), |(name, value)| (name, Some(value)));
        if names.contains(&name) {
            if let Some(value) = inline {
                return Some(value.to_owned());
            }
            return args
                .get(index + 1)
                .map(|value| value.to_string_lossy().into_owned());
        }
        index += 1;
    }
    None
}

/// Reads the single bare argument, skipping the values of the listed flags.
fn positional_argument(args: &[OsString], flags: &[&str]) -> Option<String> {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg.as_ref(), None), |(name, value)| (name, Some(value)));
        if flags.contains(&name) {
            if inline.is_none() {
                index += 1;
            }
        } else if !arg.starts_with('-') {
            return Some(arg.into_owned());
        }
        index += 1;
    }
    None
}
