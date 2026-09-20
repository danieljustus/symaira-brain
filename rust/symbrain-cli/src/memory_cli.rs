//! Native `symbrain memory` CLI implementation.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use crate::go_json;
use chrono::{DateTime, FixedOffset, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use serde::Serialize;
use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_memory::Store;

/// The shipped `symbrain memory` help, as printed when no subcommand is given.
const MEMORY_USAGE: &str = r"symbrain memory — embedded memory store operations

Usage:
  symbrain memory <subcommand> [flags]

Subcommands:
  list        List stored memories (optionally filtered by scope)
  search      Search memories by semantic relevance
  set         Store a memory (requires --kind)
  delete      Remove a memory by id
  rules       List procedural rules
  query-log   Inspect the memory retrieval log
  sync        Synchronize memories with a remote memory server
  serve       Run the memory HTTP API as a sync peer for 'memory sync --remote'

Use --output table|json (or --json) for the result format. Read commands
accept --scope/-s, --limit/-l, and --db; search takes one query argument.
Run 'symbrain memory <subcommand> --help' for details.
For remote synchronization, run 'symbrain memory sync --help'.
To act as the remote peer for another machine's sync, run 'symbrain memory serve --help'.
";

/// The shipped `symbrain memory list` help (`list --help`, also stdout with exit 0).
const MEMORY_LIST_USAGE: &str = r"symbrain memory list — list stored memories

Usage:
  symbrain memory list [flags]

Flags:
  --scope, -s <scope>  Filter by scope: global, project, agent, user, or session.
  --limit, -l <N>      Maximum memories to return (default 100, max 1000).
  --db <path>          Database path override.
  --output table|json   Output format (default table; global flag).
";

/// The shipped `symbrain memory sync` help. The Go source builds one line of it
/// by concatenating the token environment-variable constant; this is the
/// rendered text.
const MEMORY_SYNC_USAGE: &str = r"symbrain memory sync — synchronize the embedded memory store with a remote

Usage:
  symbrain memory sync --remote <url> [flags]

Flags:
  --remote <url>          Base URL of the remote memory server (required).
                          Must use https, except http://localhost or
                          127.0.0.1 for local development.
  --pull                  Only pull remote changes into the local database.
  --push                  Only push local changes to the remote server.
                          (With neither flag, both directions run.)
  --token <token>         Bearer token for the remote API. May come from
                          $SYMBRAIN_MEMORY_SYNC_TOKEN instead; never pass it on the command line in
                          shared shells.
  --encrypted-relay       Exchange client-side AES-256-GCM encrypted blobs
                          through the remote /api/sync/relay endpoint, so the
                          relay never sees plaintext memory content.
  --relay-passphrase <p>  Passphrase for --encrypted-relay. May come from
                          $SYMBRAIN_MEMORY_SYNC_RELAY_PASSPHRASE instead. Both peers must share it.
  --allow-insecure-http   Override the https requirement for non-loopback
                          remotes. Bearer tokens will be sent in the clear;
                          a WARNING is printed. Use only for testing.
  --db <path>             Database path override (default: the standard
                          memory database under the XDG data directory).
  --timeout <duration>    Per-run HTTP timeout (default 60s).

The local database and its per-remote sync cursors are reused in place;
no export or import step is needed.
";

/// The line the shipped `memory sync` prints before its help when `--remote` is
/// missing.
const MEMORY_SYNC_REMOTE_REQUIRED: &str = "symbrain memory sync: --remote is required\n";

/// Hand-written usage errors. The shipped implementation prints these itself
/// rather than through the flag package, so they are portable byte for byte.
const MEMORY_SEARCH_USAGE_ERROR: &str =
    "usage: symbrain memory search <query> [--scope <scope>] [--limit <N>] [--db <path>]\n";
const MEMORY_SET_USAGE_ERROR: &str =
    "usage: symbrain memory set <content> --kind <kind> [--scope <scope>] [flags]\n";
const MEMORY_DELETE_USAGE_ERROR: &str = "usage: symbrain memory delete <id> [--db <path>]\n";

pub(crate) fn requires_go_fallback(args: &[OsString]) -> bool {
    // The usage and help shapes are decided first, because the shipped
    // implementation writes them itself and exits without touching the store or
    // the configuration. Deciding them after the preconditions below would make
    // them depend on a database path that they never open.
    let Some(verb) = args.first().map(|arg| arg.to_string_lossy().into_owned()) else {
        // `symbrain memory` with no subcommand prints its own help text and
        // never opens a store.
        return false;
    };
    let rest = args.get(1..).unwrap_or(&[]);
    match verb.as_str() {
        "-h" | "--help" | "help" => return false,
        // `list --help` prints the subcommand help to stdout with exit 0.
        "list" | "rules" | "query-log"
            if rest
                .first()
                .is_some_and(|arg| matches!(arg.to_string_lossy().as_ref(), "-h" | "--help")) =>
        {
            return false;
        }
        // With no arguments at all there is nothing for the flag package to
        // reject, and the shipped implementation prints a hand-written usage
        // line that the native side reproduces byte for byte.
        "search" | "set" | "delete" | "sync" if rest.is_empty() => return false,
        _ => {}
    }

    // A dynamic memory configuration changes the database and retrieval
    // settings, so it goes to Go.
    if memory_config_is_dynamic() {
        return true;
    }
    // An unusable database path produces the shipped error chain
    // ("failed to open sqlite database: failed to create database directory:
    // mkdir …"), which the native side does not reproduce, so that case keeps
    // the shipped bytes.
    if !database_path_is_usable(&resolve_db_path(extract_db_override(rest).as_deref())) {
        return true;
    }
    match verb.as_str() {
        "list" | "rules" | "query-log" => !read_arguments_are_allowed(rest),
        // Native for exactly one query argument plus the read flags; the flag
        // errors stay with the Go flag package.
        "search" => !search_arguments_are_allowed(rest),
        // Native for exactly one bare identifier plus `--db`.
        "delete" => !delete_arguments_are_allowed(rest),
        "set" => !set_arguments_are_allowed(rest),
        // Anything left over names a remote and is real synchronisation work,
        // so it keeps the shipped implementation.
        _ => true,
    }
}

/// Reads the shipped memory configuration lookups that change the database or
/// the retrieval behaviour.
fn memory_config_is_dynamic() -> bool {
    if std::env::vars_os().any(|(name, _)| name.to_string_lossy().starts_with("SYMMEMORY_")) {
        return true;
    }
    let config_root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| symbrain_core::xdg::home_dir().map(|home| home.join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    config_root.join("symmemory/config.toml").is_file()
        || std::env::current_dir().is_ok_and(|dir| dir.join(".symmemory.toml").is_file())
}

/// Reports whether the resolved database path can be opened.
///
/// A missing database is fine as long as its directory (or the nearest
/// existing ancestor) is writable; otherwise the shipped implementation's
/// directory-creation error is the contract, and the command stays on Go.
fn database_path_is_usable(path: &std::path::Path) -> bool {
    if path.is_file() {
        return true;
    }
    let mut current = path.parent();
    while let Some(directory) = current {
        match std::fs::metadata(directory) {
            Ok(metadata) => return is_writable(&metadata),
            Err(_) => current = directory.parent(),
        }
    }
    false
}

#[cfg(unix)]
fn is_writable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o200 != 0
}

#[cfg(not(unix))]
fn is_writable(metadata: &std::fs::Metadata) -> bool {
    !metadata.permissions().readonly()
}

/// Accepts the shipped `memory set` shape only when every value is one the
/// native store writes identically: one content argument, a valid `--kind`,
/// an optional valid `--scope`, plus `--staged` and `--db`. Anything else -
/// a missing or unknown kind, `--author`/`--metadata`/`--entities`, an empty
/// content - keeps the shipped error text and stays on Go.
fn set_arguments_are_allowed(args: &[OsString]) -> bool {
    const KINDS: [&str; 4] = ["user", "feedback", "project", "reference"];
    const SCOPES: [&str; 5] = ["global", "project", "agent", "user", "session"];
    let mut kind: Option<String> = None;
    let mut scope: Option<String> = None;
    let mut content: Option<String> = None;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg.as_ref(), None), |(name, value)| (name, Some(value)));
        match name {
            "-kind" | "--kind" | "-k" => kind = flag_argument(args, &mut index, inline),
            "-scope" | "--scope" | "-s" => scope = flag_argument(args, &mut index, inline),
            "-db" | "--db" => {
                if flag_argument(args, &mut index, inline).is_none() {
                    return false;
                }
            }
            "--staged" => {}
            _ if !arg.starts_with('-') => {
                if content.is_some() {
                    return false;
                }
                content = Some(arg.into_owned());
            }
            _ => return false,
        }
        index += 1;
    }
    let Some(kind) = kind else {
        return false;
    };
    if !KINDS.contains(&kind.as_str()) {
        return false;
    }
    if scope.is_some_and(|scope| !SCOPES.contains(&scope.as_str())) {
        return false;
    }
    content.is_some_and(|content| !content.trim().is_empty())
}

/// Reads a flag's value from `--flag=value` or the following argument.
fn flag_argument(args: &[OsString], index: &mut usize, inline: Option<&str>) -> Option<String> {
    if let Some(value) = inline {
        return Some(value.to_owned());
    }
    *index += 1;
    args.get(*index)
        .map(|value| value.to_string_lossy().into_owned())
}

/// Accepts `--db` plus exactly one bare identifier, the shipped `memory
/// delete` shape.
fn delete_arguments_are_allowed(args: &[OsString]) -> bool {
    let mut positionals = 0;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg.as_ref(), None), |(name, value)| (name, Some(value)));
        match name {
            "-db" | "--db" => {
                if inline.is_none() {
                    if index + 1 >= args.len() {
                        return false;
                    }
                    index += 1;
                }
            }
            _ if !arg.starts_with('-') => positionals += 1,
            _ => return false,
        }
        index += 1;
    }
    positionals == 1
}

/// Accepts exactly the flag shapes the shipped read commands define:
/// `--scope`/`-s`, `--limit`/`-l` and `--db`, each with a value. A bare
/// argument is rejected, because the shipped flag sets report positionals as
/// unexpected.
fn read_arguments_are_allowed(args: &[OsString]) -> bool {
    let allowed = [
        "-scope", "--scope", "-s", "-limit", "--limit", "-l", "-actor", "--actor", "-db", "--db",
    ];
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg.as_ref(), None), |(name, value)| (name, Some(value)));
        if !allowed.contains(&name) {
            return false;
        }
        if inline.is_none() {
            if index + 1 >= args.len() {
                return false;
            }
            index += 1;
        }
        index += 1;
    }
    true
}

/// Accepts exactly the flag shapes the shipped `memory search` defines:
/// one query argument plus `--scope`/`-s`, `--limit`/`-l` and `--db`, each
/// with a value. Everything else - a missing query, a second positional, an
/// unknown flag - keeps the shipped usage or flag error.
fn search_arguments_are_allowed(args: &[OsString]) -> bool {
    let allowed = [
        "-scope", "--scope", "-s", "-limit", "--limit", "-l", "-db", "--db",
    ];
    let mut positionals = 0;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg.as_ref(), None), |(name, value)| (name, Some(value)));
        if !arg.starts_with('-') {
            positionals += 1;
            index += 1;
            continue;
        }
        if !allowed.contains(&name) {
            return false;
        }
        if inline.is_none() {
            if index + 1 >= args.len() {
                return false;
            }
            index += 1;
        }
        index += 1;
    }
    positionals == 1
}

/// Resolves the memory database the native commands open.
///
/// Mirrors the shipped implementation: the configured/`--db` override wins,
/// then the current `<data>/memory/default.db`, then a legacy
/// `~/.local/share/symmemory` installation. `SYMBRAIN_MEMORY_DB_PATH` is a
/// Rust-side test hook and has no counterpart in the shipped CLI.
fn resolve_db_path(override_path: Option<&str>) -> PathBuf {
    if let Some(path) = override_path.filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("SYMBRAIN_MEMORY_DB_PATH") {
        return PathBuf::from(path);
    }
    let current = symbrain_core::xdg::data_dir().map(|dir| dir.join("memory"));
    let legacy = symbrain_core::xdg::home_dir().map(|home| home.join(".local/share/symmemory"));
    let directory = match (&current, &legacy) {
        (Some(current), Some(legacy)) if !current.exists() && legacy.exists() => legacy.clone(),
        (Some(current), _) => current.clone(),
        (None, Some(legacy)) => legacy.clone(),
        (None, None) => PathBuf::from(".memory"),
    };
    directory.join("default.db")
}

/// Reads the `--db` override shared by every memory subcommand.
fn extract_db_override(args: &[OsString]) -> Option<String> {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        if arg == "-db" || arg == "--db" {
            return args
                .get(index + 1)
                .map(|value| value.to_string_lossy().into_owned());
        }
        for prefix in ["-db=", "--db="] {
            if let Some(value) = arg.strip_prefix(prefix) {
                return Some(value.to_owned());
            }
        }
        index += 1;
    }
    None
}

/// Runs `symbrain memory`.
pub fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    if args.is_empty() {
        let _ = write!(stderr, "{MEMORY_USAGE}");
        return exit::USAGE;
    }

    let verb = args[0].to_string_lossy();
    let rest = &args[1..];

    match verb.as_ref() {
        "-h" | "--help" | "help" => {
            let _ = write!(stdout, "{MEMORY_USAGE}");
            exit::OK
        }
        "list" => run_list(rest, stdout, stderr, format),
        "search" => run_search(rest, stdout, stderr, format),
        "set" => run_set(rest, stdout, stderr, format),
        "delete" => run_delete(rest, stdout, stderr, format),
        "rules" => run_rules(rest, stdout, stderr, format),
        "query-log" => run_query_log(rest, stdout, stderr, format),
        // The bare `memory sync` shape. The shipped implementation refuses
        // without a remote and prints its help; real synchronisation work never
        // arrives here, because the gate keeps it on Go.
        "sync" => {
            let _ = write!(stderr, "{MEMORY_SYNC_REMOTE_REQUIRED}{MEMORY_SYNC_USAGE}");
            exit::USAGE
        }
        _ => {
            let _ = writeln!(stderr, "symbrain memory: unknown subcommand {verb:?}\n");
            let _ = write!(stderr, "{MEMORY_USAGE}");
            exit::USAGE
        }
    }
}

fn open_store(stderr: &mut dyn Write, override_path: Option<&str>) -> Result<Store, u8> {
    let db_path = resolve_db_path(override_path);
    Store::open(&db_path).map_err(|err| {
        let _ = writeln!(stderr, "symbrain memory: open database: {err}");
        exit::GENERIC
    })
}

fn run_list(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    // `memory list --help` prints the subcommand help to stdout with exit 0.
    if args
        .first()
        .is_some_and(|arg| matches!(arg.to_string_lossy().as_ref(), "-h" | "--help"))
    {
        let _ = write!(stdout, "{MEMORY_LIST_USAGE}");
        return exit::OK;
    }

    let mut scope: Option<String> = None;
    // The shipped flag defaults: no scope, and a limit of 0 that the scan
    // turns into 1000 rows.
    let mut limit = 0;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "-scope" || arg == "--scope" || arg == "-s" {
            if i + 1 < args.len() {
                scope = Some(args[i + 1].to_string_lossy().into_owned());
                i += 2;
                continue;
            }
        } else if let Some(v) = arg
            .strip_prefix("-scope=")
            .or_else(|| arg.strip_prefix("--scope="))
            .or_else(|| arg.strip_prefix("-s="))
        {
            scope = Some(v.to_string());
            i += 1;
            continue;
        } else if arg == "-limit" || arg == "--limit" || arg == "-l" {
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
            .or_else(|| arg.strip_prefix("-l="))
        {
            if let Ok(l) = v.parse() {
                limit = l;
            }
            i += 1;
            continue;
        }
        i += 1;
    }

    let store = match open_store(stderr, extract_db_override(args).as_deref()) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let rows = match store.list_lite(scope.as_deref().unwrap_or(""), limit) {
        Ok(rows) => rows,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain memory list: list memories: {err}");
            return exit::GENERIC;
        }
    };

    match format {
        OutputFormat::Json => {
            let rendered = rows
                .iter()
                .map(symbrain_memory::MemoryListRow::to_go_json)
                .collect::<Vec<_>>()
                .join(",");
            let _ = writeln!(stdout, "[{rendered}]");
        }
        OutputFormat::Table => {
            if rows.is_empty() {
                let _ = writeln!(stdout, "No memories found.");
            } else {
                let _ = writeln!(stdout, "ID\tSCOPE\tCREATED\tCONTENT");
                for row in &rows {
                    let created = row
                        .created_at
                        .map(|time| time.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                        .unwrap_or_default();
                    let _ = writeln!(
                        stdout,
                        "{}\t{}\t{}\t{}",
                        row.id,
                        row.scope,
                        created,
                        table_content(&row.content)
                    );
                }
            }
        }
    }

    exit::OK
}

/// Collapses tab, carriage return and newline so one memory stays on one row.
fn table_content(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ")
}

/// The flag surface the shipped `memory search` defines.
struct SearchArguments {
    query: Option<String>,
    scope: String,
    limit: i64,
    positionals: usize,
}

/// Parses `--scope`/`-s`, `--limit`/`-l` and bare positionals exactly the way
/// the shipped flag set does (values after the flag, or after `=`).
fn parse_search_arguments(args: &[OsString]) -> SearchArguments {
    let mut parsed = SearchArguments {
        query: None,
        scope: String::new(),
        limit: 0,
        positionals: 0,
    };
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        if arg == "-scope" || arg == "--scope" || arg == "-s" {
            if index + 1 < args.len() {
                parsed.scope = args[index + 1].to_string_lossy().into_owned();
                index += 2;
                continue;
            }
        } else if let Some(value) = arg
            .strip_prefix("-scope=")
            .or_else(|| arg.strip_prefix("--scope="))
            .or_else(|| arg.strip_prefix("-s="))
        {
            parsed.scope = value.to_string();
            index += 1;
            continue;
        } else if arg == "-db" || arg == "--db" {
            if index + 1 < args.len() {
                index += 2;
                continue;
            }
        } else if arg
            .strip_prefix("-db=")
            .or_else(|| arg.strip_prefix("--db="))
            .is_some()
        {
            index += 1;
            continue;
        } else if arg == "-limit" || arg == "--limit" || arg == "-l" {
            if index + 1 < args.len() {
                if let Ok(value) = args[index + 1].to_string_lossy().parse() {
                    parsed.limit = value;
                }
                index += 2;
                continue;
            }
        } else if let Some(value) = arg
            .strip_prefix("-limit=")
            .or_else(|| arg.strip_prefix("--limit="))
            .or_else(|| arg.strip_prefix("-l="))
        {
            if let Ok(parsed_limit) = value.parse() {
                parsed.limit = parsed_limit;
            }
            index += 1;
            continue;
        } else if !arg.starts_with('-') {
            parsed.positionals += 1;
            if parsed.query.is_none() {
                parsed.query = Some(arg.into_owned());
            }
            index += 1;
            continue;
        }
        index += 1;
    }
    parsed
}

fn run_search(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        let _ = write!(stdout, "{MEMORY_SEARCH_USAGE}");
        return exit::OK;
    }

    let parsed = parse_search_arguments(args);
    let mut limit = parsed.limit;
    let scope = parsed.scope;
    let positionals = parsed.positionals;
    let query = parsed.query;
    if positionals != 1 {
        let _ = write!(stderr, "{MEMORY_SEARCH_USAGE_ERROR}");
        return exit::USAGE;
    }
    let query = query.unwrap_or_default();
    if query.is_empty() {
        let _ = writeln!(stderr, "symbrain memory search: query is required");
        return exit::USAGE;
    }
    if limit <= 0 {
        limit = 5;
    }

    let store = match open_store(stderr, extract_db_override(args).as_deref()) {
        Ok(store) => store,
        Err(code) => return code,
    };

    // The shipped CLI generates the query vector with the default embedding
    // configuration (Ollama first, hash fallback) and searches the matching
    // embedding space.
    let embedding = symbrain_memory::EmbeddingGenerator::default().generate(&query);
    let mut hits = match store.search_ranked(
        &embedding.vector,
        &embedding.source,
        scope.as_str(),
        usize::try_from(limit).unwrap_or(usize::MAX),
    ) {
        Ok(hits) => hits,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain memory search: search memories: {err}");
            return exit::GENERIC;
        }
    };
    // The shipped CLI redacts at the response boundary, on top of the
    // write-time redaction.
    for hit in &mut hits {
        hit.redact();
    }

    match format {
        OutputFormat::Json => {
            let rendered = hits
                .iter()
                .map(symbrain_memory::SearchHit::to_go_json)
                .collect::<Vec<_>>()
                .join(",");
            let _ = writeln!(stdout, "{}", escape_html(&format!("[{rendered}]")));
        }
        OutputFormat::Table => {
            if hits.is_empty() {
                let _ = writeln!(stdout, "No relevant memories found.");
            } else {
                let _ = writeln!(stdout, "SCORE\tID\tSCOPE\tCREATED\tCONTENT");
                for hit in &hits {
                    let _ = writeln!(
                        stdout,
                        "{:.6}\t{}\t{}\t{}\t{}",
                        f64::from(hit.score),
                        hit.memory.id,
                        hit.memory.scope,
                        hit.memory.created_at_seconds(),
                        table_content(&hit.memory.content)
                    );
                }
            }
        }
    }

    exit::OK
}

/// Escapes `&`, `<`, `>` and the two JavaScript line separators the way Go's
/// `json.Encoder` does by default.
fn escape_html(json: &str) -> String {
    json.replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn run_set(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let mut content = String::new();
    let mut scope = "global".to_string();
    let mut kind = String::new();
    let mut staged = false;
    let mut positionals = 0usize;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "-scope" || arg == "--scope" || arg == "-s" {
            if i + 1 < args.len() {
                scope = args[i + 1].to_string_lossy().into_owned();
                i += 2;
                continue;
            }
        } else if let Some(v) = arg
            .strip_prefix("-scope=")
            .or_else(|| arg.strip_prefix("--scope="))
        {
            scope = v.to_string();
            i += 1;
            continue;
        } else if arg == "-kind" || arg == "--kind" || arg == "-k" {
            if i + 1 < args.len() {
                kind = args[i + 1].to_string_lossy().into_owned();
                i += 2;
                continue;
            }
        } else if let Some(v) = arg
            .strip_prefix("-kind=")
            .or_else(|| arg.strip_prefix("--kind="))
        {
            kind = v.to_string();
            i += 1;
            continue;
        } else if arg == "--staged" {
            staged = true;
            i += 1;
            continue;
        } else if !arg.starts_with('-') {
            positionals += 1;
            if content.is_empty() {
                content = arg.into_owned();
            }
        }
        i += 1;
    }

    // The shipped implementation requires exactly one positional argument and
    // checks that before it looks at the content, so a bare `memory set` prints
    // the usage line rather than "content is required".
    if positionals != 1 {
        let _ = write!(stderr, "{MEMORY_SET_USAGE_ERROR}");
        return exit::USAGE;
    }
    let content = content.trim().to_string();
    if content.is_empty() {
        let _ = writeln!(stderr, "symbrain memory set: content is required");
        return exit::USAGE;
    }

    let store = match open_store(stderr, extract_db_override(args).as_deref()) {
        Ok(s) => s,
        Err(code) => return code,
    };

    // The shipped CLI records `cli:symbrain` as the actor; the MCP surface
    // records `mcp`.
    let options = symbrain_memory::SetOptions {
        actor: "cli:symbrain".to_owned(),
        staged,
        ..symbrain_memory::SetOptions::default()
    };
    match store.set_with_options(&content, &scope, &kind, serde_json::Map::new(), &options) {
        Ok(mem) => {
            match format {
                OutputFormat::Json => {
                    // Shipped shape: compact, `id`, `scope`, `kind`, `staged`.
                    let _ = writeln!(
                        stdout,
                        "{{\"id\":{},\"scope\":{},\"kind\":{},\"staged\":{}}}",
                        go_json_string(&mem.id),
                        go_json_string(&mem.scope),
                        go_json_string(&mem.kind),
                        staged
                    );
                }
                OutputFormat::Table => {
                    let state = if staged {
                        "staged for review"
                    } else {
                        "stored"
                    };
                    let _ = writeln!(
                        stdout,
                        "Memory {} ({}, {}, {}).",
                        mem.id, mem.scope, mem.kind, state
                    );
                }
            }
            exit::OK
        }
        Err(err) => {
            let _ = writeln!(stderr, "symbrain memory set: {err}");
            exit::GENERIC
        }
    }
}

fn run_delete(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let mut id: Option<String> = None;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_string_lossy();
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg.as_ref(), None), |(name, value)| (name, Some(value)));
        if name == "-db" || name == "--db" {
            if inline.is_none() {
                index += 1;
            }
        } else if !arg.starts_with('-') {
            id = Some(arg.into_owned());
        }
        index += 1;
    }
    let Some(id) = id else {
        let _ = write!(stderr, "{MEMORY_DELETE_USAGE_ERROR}");
        return exit::USAGE;
    };
    if id.trim().is_empty() {
        let _ = writeln!(stderr, "symbrain memory delete: id is required");
        return exit::USAGE;
    }

    let store = match open_store(stderr, extract_db_override(args).as_deref()) {
        Ok(store) => store,
        Err(code) => return code,
    };

    match store.delete(&id) {
        Ok(true) => {
            match format {
                OutputFormat::Json => {
                    // Shipped shape: compact, `id` before `deleted`.
                    let _ = writeln!(
                        stdout,
                        "{{\"id\":{},\"deleted\":true}}",
                        go_json_string(&id)
                    );
                }
                OutputFormat::Table => {
                    let _ = writeln!(stdout, "Deleted memory {id}.");
                }
            }
            exit::OK
        }
        Ok(false) => {
            let _ = writeln!(stderr, "symbrain memory delete: memory not found: {id}");
            exit::GENERIC
        }
        Err(err) => {
            let _ = writeln!(stderr, "symbrain memory delete: {err}");
            exit::GENERIC
        }
    }
}

/// Renders a string as the shipped encoder does, including its HTML escaping.
fn go_json_string(value: &str) -> String {
    go_json(&value.to_owned())
}

fn run_rules(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let scope = flag_value(args, &["-scope", "--scope", "-s"]);
    let store = match open_store(stderr, extract_db_override(args).as_deref()) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let rules = match store.list_rules(scope.as_deref().unwrap_or("")) {
        Ok(rules) => rules,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain memory rules: list rules: {err}");
            return exit::GENERIC;
        }
    };

    match format {
        OutputFormat::Json => {
            let rendered = rules
                .iter()
                .map(symbrain_memory::RuleRow::to_go_json)
                .collect::<Vec<_>>()
                .join(",");
            let _ = writeln!(stdout, "[{rendered}]");
        }
        OutputFormat::Table => {
            if rules.is_empty() {
                let _ = writeln!(stdout, "No rules found.");
            } else {
                let _ = writeln!(stdout, "ID\tSCOPE\tCREATED\tCONTENT");
                for rule in &rules {
                    let created = rule
                        .created_at
                        .map(|time| time.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                        .unwrap_or_default();
                    let _ = writeln!(
                        stdout,
                        "{}\t{}\t{}\t{}",
                        rule.id,
                        rule.scope,
                        created,
                        table_content(&rule.content)
                    );
                }
            }
        }
    }

    exit::OK
}

/// Reads the value of the first matching flag, in `--flag value` or
/// `--flag=value` form.
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

const MEMORY_SEARCH_USAGE: &str = "symbrain memory search — search memories by semantic relevance

Usage:
  symbrain memory search <query> [flags]

Flags:
  --scope, -s <scope>  Filter by scope: global, project, agent, user, or session.
  --limit, -l <N>      Maximum results to return (default 5).
  --db <path>          Database path override.
  --output table|json   Output format (default table; global flag).
";

const QUERY_LOG_USAGE: &str = "symbrain memory query-log — inspect the memory retrieval log

Usage:
  symbrain memory query-log [flags]

Flags:
  --limit, -l <N>      Maximum recent entries to return (default 50, max 1000).
  --actor <name>       Filter recent entries by actor.
  --db <path>          Database path override.
  --output table|json  Output format (default table; global flag).
";

fn run_query_log(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    if args
        .iter()
        .any(|arg| matches!(arg.to_string_lossy().as_ref(), "-h" | "-help" | "--help"))
    {
        let _ = write!(stdout, "{QUERY_LOG_USAGE}");
        return exit::OK;
    }

    let options = match parse_query_log_args(args, stderr) {
        Ok(options) => options,
        Err(code) => return code,
    };
    let store = if let Some(path) = options.db_path {
        match Store::open(&path) {
            Ok(store) => store,
            Err(err) => {
                let _ = writeln!(
                    stderr,
                    "symbrain memory query-log: open memory database: {err}"
                );
                return exit::GENERIC;
            }
        }
    } else {
        match open_store(stderr, None) {
            Ok(store) => store,
            Err(code) => return code,
        }
    };
    let mut summary = match store.query_log_summary(options.limit, &options.actor) {
        Ok(summary) => summary,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain memory query-log: read query log: {err}");
            return exit::GENERIC;
        }
    };
    normalize_query_log_timestamps(&mut summary);
    render_query_log(&summary, stdout, format);
    exit::OK
}

struct QueryLogOptions {
    limit: usize,
    actor: String,
    db_path: Option<PathBuf>,
}

fn parse_query_log_args(args: &[OsString], stderr: &mut dyn Write) -> Result<QueryLogOptions, u8> {
    let mut limit = 0_i64;
    let mut actor = String::new();
    let mut db_path = None;
    let mut index = 0;
    let mut terminated = false;
    while index < args.len() {
        let argument = args[index].to_string_lossy();
        if terminated {
            return Err(query_log_unexpected(stderr, &argument));
        }
        if argument == "--" {
            terminated = true;
            index += 1;
            continue;
        }
        if !argument.starts_with('-') {
            return Err(query_log_unexpected(stderr, &argument));
        }
        let (name, inline) = argument
            .split_once('=')
            .map_or((argument.as_ref(), None), |(name, value)| {
                (name, Some(value))
            });
        let normalized = name.trim_start_matches('-');
        let takes_value = matches!(normalized, "limit" | "l" | "actor" | "db");
        if !takes_value {
            return Err(query_log_flag_error(stderr, normalized));
        }
        let value = if let Some(value) = inline {
            value.to_string()
        } else if index + 1 < args.len() {
            index += 1;
            args[index].to_string_lossy().into_owned()
        } else {
            let _ = writeln!(stderr, "flag needs an argument: -{normalized}");
            query_log_flag_usage(stderr);
            return Err(exit::USAGE);
        };
        match normalized {
            "limit" | "l" => {
                if let Ok(parsed) = value.parse::<i64>() {
                    limit = parsed;
                } else {
                    let _ = writeln!(
                        stderr,
                        "invalid value {value:?} for flag -{normalized}: parse error"
                    );
                    query_log_flag_usage(stderr);
                    return Err(exit::USAGE);
                }
            }
            "actor" => actor = value,
            "db" => db_path = Some(PathBuf::from(value)),
            _ => unreachable!(),
        }
        index += 1;
    }
    let limit = usize::try_from(if limit <= 0 { 50 } else { limit.min(1_000) }).unwrap_or(1_000);
    Ok(QueryLogOptions {
        limit,
        actor,
        db_path,
    })
}

/// Go-shaped query-log row. Field order and `omitempty` follow
/// `internal/memory/db.QueryLogEntry` so the JSON bytes match.
#[derive(Debug, Serialize)]
struct QueryLogRow {
    id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    actor: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    scope: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    session: String,
    tool: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    query_text: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    params: String,
    duration_ms: i64,
    created_at: String,
}

/// Go-shaped summary. `tool_breakdown` and `actor_breakdown` are Go maps, so
/// their keys are sorted; `pruned_count` is omitted when zero.
#[derive(Debug, Serialize)]
struct QueryLogReport {
    total_queries: i64,
    tool_breakdown: BTreeMap<String, i64>,
    actor_breakdown: BTreeMap<String, i64>,
    recent_entries: Vec<QueryLogRow>,
    /// Go omits this field when it is zero (`omitempty`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pruned_count: Option<i64>,
}

/// Rebuilds the store's summary as the Go struct, in Go field order.
fn go_query_log_report(summary: &serde_json::Value) -> QueryLogReport {
    let counts = |key: &str| {
        summary[key]
            .as_object()
            .map(|values| {
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), value.as_i64().unwrap_or_default()))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default()
    };
    let entries = summary["recent_entries"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| QueryLogRow {
                    id: row["id"].as_str().unwrap_or_default().to_owned(),
                    actor: row["actor"].as_str().unwrap_or_default().to_owned(),
                    scope: row["scope"].as_str().unwrap_or_default().to_owned(),
                    session: row["session"].as_str().unwrap_or_default().to_owned(),
                    tool: row["tool"].as_str().unwrap_or_default().to_owned(),
                    query_text: row["query_text"].as_str().unwrap_or_default().to_owned(),
                    params: row["params"].as_str().unwrap_or_default().to_owned(),
                    duration_ms: row["duration_ms"].as_i64().unwrap_or_default(),
                    created_at: row["created_at"].as_str().unwrap_or_default().to_owned(),
                })
                .collect()
        })
        .unwrap_or_default();
    QueryLogReport {
        total_queries: summary["total_queries"].as_i64().unwrap_or_default(),
        tool_breakdown: counts("tool_breakdown"),
        actor_breakdown: counts("actor_breakdown"),
        recent_entries: entries,
        pruned_count: summary["pruned_count"].as_i64().filter(|count| *count != 0),
    }
}

fn render_query_log(summary: &serde_json::Value, stdout: &mut dyn Write, format: OutputFormat) {
    match format {
        OutputFormat::Json => {
            let _ = writeln!(stdout, "{}", go_json(&go_query_log_report(summary)));
        }
        OutputFormat::Table => {
            let total = summary["total_queries"].as_i64().unwrap_or_default();
            let _ = writeln!(stdout, "Total queries: {total}\n");
            let entries = summary["recent_entries"].as_array();
            if entries.is_none_or(Vec::is_empty) {
                let _ = writeln!(stdout, "No recorded queries.");
            } else {
                let _ = writeln!(stdout, "WHEN\tTOOL\tACTOR\tMS\tQUERY");
                for entry in entries.into_iter().flatten() {
                    let _ = writeln!(
                        stdout,
                        "{}\t{}\t{}\t{}\t{}",
                        table_query_log_timestamp(entry["created_at"].as_str().unwrap_or_default()),
                        entry["tool"].as_str().unwrap_or_default(),
                        entry["actor"].as_str().unwrap_or_default(),
                        entry["duration_ms"].as_i64().unwrap_or_default(),
                        table_query_content(entry["query_text"].as_str().unwrap_or_default()),
                    );
                }
            }
        }
    }
}

fn normalize_query_log_timestamps(summary: &mut serde_json::Value) {
    if let Some(entries) = summary["recent_entries"].as_array_mut() {
        for entry in entries {
            if let Some(raw) = entry["created_at"].as_str() {
                entry["created_at"] = normalize_query_log_timestamp(raw).into();
            }
        }
    }
}

fn parse_query_log_timestamp(raw: &str) -> Option<DateTime<FixedOffset>> {
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(raw) {
        return Some(timestamp);
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f") {
        return FixedOffset::east_opt(0)?
            .from_local_datetime(&naive)
            .single();
    }
    let mut parts = raw.split_whitespace();
    let date = parts.next()?;
    let time = parts.next()?;
    let offset = parts.next()?;
    let datetime = format!("{date} {time}");
    let naive = NaiveDateTime::parse_from_str(&datetime, "%Y-%m-%d %H:%M:%S%.f").ok()?;
    let sign = match offset.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let hours = offset.get(1..3)?.parse::<i32>().ok()?;
    let minutes = offset.get(3..5)?.parse::<i32>().ok()?;
    let offset = FixedOffset::east_opt(sign * (hours * 3_600 + minutes * 60))?;
    offset.from_local_datetime(&naive).single()
}

fn normalize_query_log_timestamp(raw: &str) -> String {
    parse_query_log_timestamp(raw).map_or_else(
        || raw.to_string(),
        |timestamp| {
            timestamp
                .with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::AutoSi, true)
        },
    )
}

fn table_query_log_timestamp(raw: &str) -> String {
    parse_query_log_timestamp(raw).map_or_else(
        || raw.to_string(),
        |timestamp| {
            timestamp
                .with_timezone(&Utc)
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string()
        },
    )
}

fn table_query_content(content: &str) -> String {
    content.replace(['\t', '\r', '\n'], " ")
}

fn query_log_unexpected(stderr: &mut dyn Write, argument: &str) -> u8 {
    let _ = writeln!(
        stderr,
        "symbrain memory query-log: unexpected argument {argument:?}"
    );
    exit::USAGE
}

fn query_log_flag_error(stderr: &mut dyn Write, normalized: &str) -> u8 {
    let _ = writeln!(stderr, "flag provided but not defined: -{normalized}");
    query_log_flag_usage(stderr);
    exit::USAGE
}

fn query_log_flag_usage(stderr: &mut dyn Write) {
    let _ = write!(
        stderr,
        "Usage of memory query-log:\n  -actor string\n    \tfilter recent entries by actor\n  -db string\n    \tdatabase path override (default: the configured memory database)\n  -l int\n    \tmaximum recent entries to return\n  -limit int\n    \tmaximum recent entries to return\n"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use serde_json::json;
    use tempfile::tempdir;

    fn fixture() -> tempfile::TempDir {
        let directory = tempdir().expect("fixture directory");
        let path = directory.path().join("memory.sqlite");
        let store = Store::open(&path).expect("initialize fixture database");
        drop(store);
        let connection = Connection::open(path).expect("open fixture database");
        connection
            .execute_batch(
                "INSERT INTO query_log(id,actor,scope,session,tool,query_text,params,duration_ms,created_at) VALUES
                ('q1','claude','global','s1','memory_search','tabs',NULL,12,'2026-03-01 00:00:00 +0000 UTC'),
                ('q2','codex','global','s2','memory_list',NULL,NULL,3,'2026-03-01 00:01:00 +0000 UTC'),
                ('q3',NULL,NULL,NULL,'memory_search','legacy','{}',7,'2026-03-01 00:02:00.123456 +0000 UTC');",
            )
            .expect("seed fixture database");
        directory
    }

    #[test]
    fn query_log_json_matches_go_fixture() {
        let directory = fixture();
        let path = directory.path().join("memory.sqlite");
        let args = [OsString::from("--db"), path.into_os_string()];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            run_query_log(&args, &mut stdout, &mut stderr, OutputFormat::Json),
            exit::OK
        );
        assert!(stderr.is_empty());
        let actual: serde_json::Value = serde_json::from_slice(&stdout).expect("valid JSON");
        assert_eq!(
            actual,
            json!({
                "total_queries": 3,
                "tool_breakdown": {"memory_list": 1, "memory_search": 2},
                "actor_breakdown": {"(unknown)": 1, "claude": 1, "codex": 1},
                "recent_entries": [
                    {"id": "q3", "tool": "memory_search", "query_text": "legacy", "params": "{}", "duration_ms": 7, "created_at": "2026-03-01T00:02:00.123456Z"},
                    {"id": "q2", "actor": "codex", "scope": "global", "session": "s2", "tool": "memory_list", "duration_ms": 3, "created_at": "2026-03-01T00:01:00Z"},
                    {"id": "q1", "actor": "claude", "scope": "global", "session": "s1", "tool": "memory_search", "query_text": "tabs", "duration_ms": 12, "created_at": "2026-03-01T00:00:00Z"}
                ]
            })
        );
    }

    #[test]
    fn query_log_table_and_actor_filter_match_go_shape() {
        let directory = fixture();
        let path = directory.path().join("memory.sqlite");
        let args = [
            OsString::from("--db"),
            path.into_os_string(),
            OsString::from("--actor"),
            OsString::from("codex"),
            OsString::from("--limit=99999"),
        ];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            run_query_log(&args, &mut stdout, &mut stderr, OutputFormat::Table),
            exit::OK
        );
        assert!(stderr.is_empty());
        assert_eq!(
            String::from_utf8(stdout).expect("UTF-8 table"),
            "Total queries: 1\n\nWHEN\tTOOL\tACTOR\tMS\tQUERY\n2026-03-01T00:01:00Z\tmemory_list\tcodex\t3\t\n"
        );
    }

    #[test]
    fn query_log_rejects_positionals_and_unknown_flags() {
        for args in [
            [OsString::from("unexpected")],
            [OsString::from("--nonsense")],
        ] {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            assert_eq!(
                run_query_log(&args, &mut stdout, &mut stderr, OutputFormat::Table),
                exit::USAGE
            );
            assert!(stdout.is_empty());
            assert!(!stderr.is_empty());
        }
    }

    #[test]
    fn query_log_table_sanitizes_multiline_query_text() {
        let directory = fixture();
        let path = directory.path().join("memory.sqlite");
        let connection = Connection::open(&path).expect("open fixture database");
        connection
            .execute(
                "INSERT INTO query_log(id,tool,query_text,duration_ms,created_at) VALUES (?, ?, ?, ?, ?)",
                (
                    "q4",
                    "memory_search",
                    "line one\tline two\rline three\nline four",
                    4,
                    "2026-03-01T00:03:00Z",
                ),
            )
            .expect("seed multiline query");
        drop(connection);

        let args = [OsString::from("--db"), path.into_os_string()];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            run_query_log(&args, &mut stdout, &mut stderr, OutputFormat::Table),
            exit::OK
        );
        let output = String::from_utf8(stdout).expect("UTF-8 table");
        assert!(output.contains("line one line two line three line four"));
        assert!(!output.contains("line one\tline two"));
        assert!(!output.contains("line three\nline four"));
    }

    #[test]
    fn query_log_normalizes_schema_default_timestamp() {
        let directory = fixture();
        let path = directory.path().join("memory.sqlite");
        let connection = Connection::open(&path).expect("open fixture database");
        connection
            .execute(
                "INSERT INTO query_log(id,tool,query_text,duration_ms,created_at) VALUES (?, ?, ?, ?, ?)",
                (
                    "q5",
                    "memory_list",
                    "schema default",
                    5,
                    "2026-03-01 00:03:00",
                ),
            )
            .expect("seed schema-default timestamp");
        drop(connection);

        let args = [OsString::from("--db"), path.into_os_string()];
        let mut json_output = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            run_query_log(&args, &mut json_output, &mut stderr, OutputFormat::Json),
            exit::OK
        );
        let summary: serde_json::Value =
            serde_json::from_slice(&json_output).expect("valid JSON summary");
        assert_eq!(
            summary["recent_entries"][0]["created_at"],
            "2026-03-01T00:03:00Z"
        );

        let mut table_output = Vec::new();
        assert_eq!(
            run_query_log(&args, &mut table_output, &mut stderr, OutputFormat::Table),
            exit::OK
        );
        assert!(
            String::from_utf8(table_output)
                .expect("UTF-8 table")
                .contains("2026-03-01T00:03:00Z\tmemory_list")
        );
    }

    #[test]
    fn search_keeps_go_for_shapes_the_native_path_does_not_own() {
        let allowed = |args: &[&str]| {
            search_arguments_are_allowed(
                &args
                    .iter()
                    .map(|arg| OsString::from(*arg))
                    .collect::<Vec<_>>(),
            )
        };
        assert!(allowed(&["alpha"]));
        assert!(allowed(&["alpha", "--scope", "global"]));
        assert!(allowed(&["alpha", "-s=project", "--limit", "3"]));
        assert!(allowed(&["--db", "/tmp/x.db", "alpha"]));
        assert!(allowed(&["--limit=0", "two words"]));
        // Missing, doubled or flag-shaped queries, and any unknown flag, keep
        // the shipped usage or flag error.
        assert!(!allowed(&[]));
        assert!(!allowed(&["alpha", "beta"]));
        assert!(!allowed(&["alpha", "--bogus"]));
        assert!(!allowed(&["--help"]));
        assert!(!allowed(&["alpha", "--limit"]));
        assert!(!allowed(&["alpha", "--scope"]));
    }

    #[test]
    fn search_refuses_a_missing_query_with_the_shipped_message() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            run_search(&[], &mut stdout, &mut stderr, OutputFormat::Table),
            exit::USAGE
        );
        assert!(stdout.is_empty());
        assert_eq!(
            String::from_utf8(stderr).expect("UTF-8 stderr"),
            "usage: symbrain memory search <query> [--scope <scope>] [--limit <N>] [--db <path>]\n"
        );
    }

    #[test]
    fn search_prints_the_shipped_usage_for_help() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let args = [OsString::from("--help")];
        assert_eq!(
            run_search(&args, &mut stdout, &mut stderr, OutputFormat::Table),
            exit::OK
        );
        assert!(stderr.is_empty());
        assert!(
            String::from_utf8(stdout)
                .expect("UTF-8 stdout")
                .starts_with("symbrain memory search — search memories by semantic relevance\n")
        );
    }

    #[test]
    fn search_flags_consume_their_values() {
        let cases = [
            vec!["search", "alpha", "--db", "/tmp/memory.db"],
            vec!["search", "alpha", "--limit", "2", "--db", "/tmp/memory.db"],
            vec![
                "search",
                "alpha",
                "-l",
                "2",
                "-s",
                "global",
                "--db=/tmp/memory.db",
            ],
        ];
        for case in cases {
            let args = case.iter().map(OsString::from).collect::<Vec<_>>();
            assert!(
                !requires_go_fallback(&args),
                "{case:?} should stay native so the parser owns the values"
            );
        }
    }

    #[test]
    fn json_output_escapes_what_the_shipped_encoder_escapes() {
        assert_eq!(
            escape_html("a&b<c>d\u{2028}e\u{2029}"),
            "a\\u0026b\\u003cc\\u003ed\\u2028e\\u2029"
        );
    }

    /// The shapes the native path owns because the shipped implementation
    /// writes them itself instead of going through the flag package.
    #[test]
    fn hand_written_usage_shapes_stay_native() {
        let native = [
            vec![],
            vec!["-h"],
            vec!["--help"],
            vec!["help"],
            vec!["list", "--help"],
            vec!["search"],
            vec!["set"],
            vec!["delete"],
            vec!["sync"],
        ];
        for case in native {
            let args = case.iter().map(OsString::from).collect::<Vec<_>>();
            assert!(
                !requires_go_fallback(&args),
                "{case:?} is a hand-written usage or help shape and must stay native"
            );
        }
    }

    /// Real synchronisation work must never run natively: only the bare
    /// `memory sync` shape is ported, everything that names a remote keeps the
    /// shipped implementation.
    #[test]
    fn memory_sync_with_arguments_stays_on_go() {
        for case in [
            vec!["sync", "--remote", "https://example.test"],
            vec!["sync", "--pull"],
            vec!["sync", "--remote=https://example.test", "--push"],
        ] {
            let args = case.iter().map(OsString::from).collect::<Vec<_>>();
            assert!(
                requires_go_fallback(&args),
                "{case:?} carries synchronisation arguments and must go to Go"
            );
        }
    }
}
