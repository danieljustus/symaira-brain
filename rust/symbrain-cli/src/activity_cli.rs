//! Native `symbrain activity` CLI implementation.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use symbrain_activity::{
    SearchOptions, fence_summary, profile_allows_activity, validate_search_options,
};
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
    match load_profile(&name) {
        Ok(profile) => profile_allows_activity(&profile),
        Err(_) => false,
    }
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
fn run_search(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let mut query = String::new();
    let mut from: Option<DateTime<Utc>> = None;
    let mut to: Option<DateTime<Utc>> = None;
    let mut limit = 10;
    let mut max_tokens = 1000;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "-from" || arg == "--from" {
            if i + 1 < args.len() {
                from = args[i + 1].to_string_lossy().parse().ok();
                i += 2;
                continue;
            }
        } else if let Some(v) = arg
            .strip_prefix("-from=")
            .or_else(|| arg.strip_prefix("--from="))
        {
            from = v.parse().ok();
            i += 1;
            continue;
        } else if arg == "-to" || arg == "--to" {
            if i + 1 < args.len() {
                to = args[i + 1].to_string_lossy().parse().ok();
                i += 2;
                continue;
            }
        } else if let Some(v) = arg
            .strip_prefix("-to=")
            .or_else(|| arg.strip_prefix("--to="))
        {
            to = v.parse().ok();
            i += 1;
            continue;
        } else if arg == "-limit" || arg == "--limit" {
            if i + 1 < args.len() {
                if let Ok(l) = args[i + 1].to_string_lossy().parse() {
                    limit = l;
                }
                i += 2;
                continue;
            }
        } else if let Some(v) = arg
            .strip_prefix("-limit=")
            .or_else(|| arg.strip_prefix("--limit="))
        {
            if let Ok(l) = v.parse() {
                limit = l;
            }
            i += 1;
            continue;
        } else if arg == "-max-tokens" || arg == "--max-tokens" {
            if i + 1 < args.len() {
                if let Ok(t) = args[i + 1].to_string_lossy().parse() {
                    max_tokens = t;
                }
                i += 2;
                continue;
            }
        } else if let Some(v) = arg
            .strip_prefix("-max-tokens=")
            .or_else(|| arg.strip_prefix("--max-tokens="))
        {
            if let Ok(t) = v.parse() {
                max_tokens = t;
            }
            i += 1;
            continue;
        } else if arg.starts_with("--profile") || arg.starts_with("-profile") {
            if !arg.contains('=') {
                i += 1;
            }
            i += 1;
            continue;
        } else if !arg.starts_with('-') && query.is_empty() {
            query = arg.into_owned();
        }
        i += 1;
    }

    let Some(from_dt) = from else {
        let _ = writeln!(stderr, "symbrain activity search: --from required");
        return exit::USAGE;
    };
    let Some(to_dt) = to else {
        let _ = writeln!(stderr, "symbrain activity search: --to required");
        return exit::USAGE;
    };

    let search_opts = SearchOptions {
        query: query.clone(),
        source: String::new(),
        from: from_dt,
        to: to_dt,
        limit,
        max_tokens,
        include_episodes: false,
    };

    if let Err(err) = validate_search_options(&search_opts) {
        let _ = writeln!(stderr, "symbrain activity search: {err}");
        return exit::USAGE;
    }

    let db_path = resolve_db_path();
    let store = match Store::open(&db_path) {
        Ok(s) => s,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain activity: open database: {err}");
            return exit::GENERIC;
        }
    };

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
    let db_path = resolve_db_path();
    let store = match Store::open(&db_path) {
        Ok(s) => s,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain activity: open database: {err}");
            return exit::GENERIC;
        }
    };

    let status = match store.activity_status() {
        Ok(s) => s,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain activity status: {err}");
            return exit::GENERIC;
        }
    };

    match format {
        OutputFormat::Json => {
            let _ = writeln!(
                stdout,
                "{}",
                serde_json::to_string_pretty(&status).unwrap_or_default()
            );
        }
        OutputFormat::Table => {
            let _ = writeln!(stdout, "Activity Store: {}", db_path.display());
            let _ = writeln!(stdout, "Active segments: {}", status.active_segments);
            let _ = writeln!(stdout, "Active episodes: {}", status.active_episodes);
        }
    }

    exit::OK
}
