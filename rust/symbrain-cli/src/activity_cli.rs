//! Native `symbrain activity` CLI implementation.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use crate::go_json;
#[path = "activity_args.rs"]
mod activity_args;
#[path = "activity_time.rs"]
mod activity_time;
use activity_args::Args;
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
const MAX_ACTIVITY_BUDGET: i64 = 4000;
fn resolve_db_path(override_path: Option<&str>) -> PathBuf {
    if let Some(path) = override_path.filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
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
    if matches!(verb.as_ref(), "-h" | "--help") {
        let _ = write!(stdout, "{ACTIVITY_USAGE}");
        return exit::OK;
    }

    // Check profile
    let profile_name = extract_flag(args);
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
            let _ = writeln!(
                stderr,
                "symbrain activity: unknown subcommand {}",
                symbrain_core::config::format_go_quoted(&args[0])
            );
            let _ = write!(stderr, "{ACTIVITY_USAGE}");
            exit::USAGE
        }
    }
}

fn extract_flag(args: &[OsString]) -> Option<String> {
    let mut name = None;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        if arg == "--profile" && index + 1 < args.len() {
            index += 1;
            name = Some(args[index].to_string_lossy().into_owned());
        } else if let Some(value) = arg.strip_prefix("--profile=") {
            name = Some(value.to_owned());
        }
        index += 1;
    }
    name.filter(|value| !value.is_empty())
}

fn parsed_args(args: &[OsString], verb: &str, stderr: &mut dyn Write) -> Result<Args, u8> {
    let parsed = activity_args::parse(args, verb, stderr);
    let valid = parsed.as_ref().is_ok_and(|p| {
        p.positional.len() == usize::from(verb != "status")
            && (verb == "search" || (1..=MAX_ACTIVITY_BUDGET).contains(&p.budget))
    });
    if valid {
        return parsed.map_err(|()| exit::USAGE);
    }
    let usage = match verb {
        "search" => {
            "usage: symbrain activity search <query> --profile <name> --from <RFC3339> --to <RFC3339> --limit <N> --max-tokens <N> [--db <path>]"
        }
        "get" => {
            "usage: symbrain activity get <id> --profile <name> --max-tokens <N> [--db <path>]"
        }
        _ => "usage: symbrain activity status --profile <name> --max-tokens <N> [--db <path>]",
    };
    let _ = writeln!(stderr, "{usage}");
    Err(exit::USAGE)
}

#[allow(clippy::too_many_lines)]
/// Opens the activity store, reporting the shipped per-subcommand message.
fn open_store(
    stderr: &mut dyn Write,
    subcommand: &str,
    db_override: Option<&str>,
) -> Result<Store, u8> {
    let db_path = resolve_db_path(db_override);
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
    let parsed = match parsed_args(args, "search", stderr) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    if !(1..=50).contains(&parsed.limit) || !(1..=4000).contains(&parsed.budget) {
        let _ = writeln!(
            stderr,
            "symbrain activity search: invalid bounds (limit 1-50; budget 1-4000)"
        );
        return exit::USAGE;
    }
    let window = activity_time::parse(&parsed.from)
        .map_err(|e| format!("from must be RFC3339: {e}"))
        .and_then(|from| {
            activity_time::parse(&parsed.to)
                .map(|to| (from, to))
                .map_err(|e| format!("to must be RFC3339: {e}"))
        });
    let (from, to) = match window {
        Ok(window) => window,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain activity search: {error}");
            return exit::USAGE;
        }
    };
    let limit = usize::try_from(parsed.limit).expect("validated limit");
    let max_tokens = usize::try_from(parsed.budget).expect("validated budget");
    let options = SearchOptions {
        query: parsed.positional[0].clone(),
        source: String::new(),
        from,
        to,
        limit,
        max_tokens,
        include_episodes: false,
    };
    if let Err(error) = validate_search_options(&options) {
        let _ = writeln!(stderr, "symbrain activity search: {error}");
        return exit::USAGE;
    }
    let mem_search = ActivitySearch {
        query: options.query,
        source: String::new(),
        from,
        to,
        limit,
        max_tokens,
        include_episodes: false,
    };
    let store = match open_store(
        stderr,
        "search",
        Some(parsed.db.as_str()).filter(|p| !p.is_empty()),
    ) {
        Ok(store) => store,
        Err(code) => return code,
    };

    let page = match store.activity_search(&mem_search) {
        Ok(page) => page,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain activity search: search: {err}");
            return exit::GENERIC;
        }
    };

    // The shipped command fences every summary, reports its token count and
    // sums the used budget.
    let mut page = page;
    let mut used = 0;
    for item in &mut page.results {
        let summary = fence_summary(&item.summary, max_tokens);
        item.tokens = if summary.is_empty() {
            0
        } else {
            summary.chars().count() / 4 + 1
        };
        used += item.tokens;
        item.summary = summary;
    }
    page.used_tokens = used;
    page.max_tokens = max_tokens;

    match format {
        OutputFormat::Json => {
            let _ = writeln!(stdout, "{}", go_json(&page));
        }
        OutputFormat::Table => {
            for item in &page.results {
                let _ = writeln!(
                    stdout,
                    "{}\t{}\t{}",
                    item.started_at.format("%Y-%m-%dT%H:%M:%SZ"),
                    item.kind,
                    item.summary
                );
            }
        }
    }

    exit::OK
}

fn run_status(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let parsed = match parsed_args(args, "status", stderr) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let store = match open_store(
        stderr,
        "status",
        Some(parsed.db.as_str()).filter(|p| !p.is_empty()),
    ) {
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
    let parsed = match parsed_args(args, "get", stderr) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    let id = parsed.positional[0].clone();
    let max_tokens = usize::try_from(parsed.budget).expect("validated budget");
    let store = match open_store(
        stderr,
        "get",
        Some(parsed.db.as_str()).filter(|p| !p.is_empty()),
    ) {
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
    let summary = fence_summary(&item.summary, max_tokens);
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

#[cfg(test)]
#[path = "activity_tests.rs"]
mod tests;
