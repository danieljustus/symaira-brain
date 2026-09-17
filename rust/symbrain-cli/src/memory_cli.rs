//! Native `symbrain memory` CLI implementation.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

use chrono::{DateTime, FixedOffset, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use symbrain_core::exit;
use symbrain_core::output::OutputFormat;
use symbrain_memory::{Memory, Store};

const MEMORY_USAGE: &str = "symbrain memory — operate the embedded memory store

Usage:
  symbrain memory list [--scope SCOPE] [--limit N]
  symbrain memory search <query> [--scope SCOPE] [--limit N]
  symbrain memory set <id> <content> [--scope SCOPE]
  symbrain memory delete <id>
  symbrain memory rules
  symbrain memory query-log [--limit N] [--actor NAME] [--db PATH]

The global --output table|json flag (or --json) selects the output format.
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
    if let Some(home) = symbrain_core::xdg::home_dir() {
        let legacy = home
            .join(".local")
            .join("share")
            .join("symmemory")
            .join("memory.db");
        if legacy.exists() {
            return legacy;
        }
    }
    symbrain_core::xdg::data_dir().map_or_else(
        || PathBuf::from(".memory.db"),
        |d| d.join("memory").join("memory.db"),
    )
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
        _ => {
            let _ = writeln!(stderr, "symbrain memory: unknown subcommand {verb:?}\n");
            let _ = write!(stderr, "{MEMORY_USAGE}");
            exit::USAGE
        }
    }
}

fn open_store(stderr: &mut dyn Write) -> Result<Store, u8> {
    let db_path = resolve_db_path();
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
    let mut scope: Option<String> = None;
    let mut limit = 50;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "-scope" || arg == "--scope" {
            if i + 1 < args.len() {
                scope = Some(args[i + 1].to_string_lossy().into_owned());
                i += 2;
                continue;
            }
        } else if let Some(v) = arg
            .strip_prefix("-scope=")
            .or_else(|| arg.strip_prefix("--scope="))
        {
            scope = Some(v.to_string());
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
        }
        i += 1;
    }

    let store = match open_store(stderr) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let memories = match store.list(scope.as_deref().unwrap_or(""), limit) {
        Ok(m) => m,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain memory list: {err}");
            return exit::GENERIC;
        }
    };

    match format {
        OutputFormat::Json => {
            let _ = writeln!(
                stdout,
                "{}",
                serde_json::to_string_pretty(&memories).unwrap_or_default()
            );
        }
        OutputFormat::Table => {
            if memories.is_empty() {
                let _ = writeln!(stdout, "No memories found.");
            } else {
                let _ = writeln!(stdout, "ID\tSCOPE\tUPDATED\tCONTENT");
                for m in &memories {
                    let preview = m.content.lines().next().unwrap_or("");
                    let short_preview = if preview.len() > 60 {
                        &preview[..60]
                    } else {
                        preview
                    };
                    let _ = writeln!(
                        stdout,
                        "{}\t{}\t{}\t{}",
                        m.id,
                        m.scope,
                        m.updated_at.format("%Y-%m-%d %H:%M"),
                        short_preview
                    );
                }
            }
        }
    }

    exit::OK
}

fn run_search(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let mut query = String::new();
    let mut scope: Option<String> = None;
    let mut limit = 20;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "-scope" || arg == "--scope" {
            if i + 1 < args.len() {
                scope = Some(args[i + 1].to_string_lossy().into_owned());
                i += 2;
                continue;
            }
        } else if let Some(v) = arg
            .strip_prefix("-scope=")
            .or_else(|| arg.strip_prefix("--scope="))
        {
            scope = Some(v.to_string());
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
        } else if !arg.starts_with('-') && query.is_empty() {
            query = arg.into_owned();
        }
        i += 1;
    }

    if query.is_empty() {
        let _ = writeln!(stderr, "symbrain memory search: query required");
        return exit::USAGE;
    }

    let store = match open_store(stderr) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let results = match store.search(&query, scope.as_deref().unwrap_or(""), limit) {
        Ok(r) => r,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain memory search: {err}");
            return exit::GENERIC;
        }
    };

    match format {
        OutputFormat::Json => {
            let list: Vec<&Memory> = results.iter().map(|(m, _)| m).collect();
            let _ = writeln!(
                stdout,
                "{}",
                serde_json::to_string_pretty(&list).unwrap_or_default()
            );
        }
        OutputFormat::Table => {
            if results.is_empty() {
                let _ = writeln!(stdout, "No matching memories found.");
            } else {
                let _ = writeln!(stdout, "SCORE\tID\tSCOPE\tCONTENT");
                for (m, score) in &results {
                    let preview = m.content.lines().next().unwrap_or("");
                    let short_preview = if preview.len() > 60 {
                        &preview[..60]
                    } else {
                        preview
                    };
                    let _ = writeln!(
                        stdout,
                        "{:.2}\t{}\t{}\t{}",
                        score, m.id, m.scope, short_preview
                    );
                }
            }
        }
    }

    exit::OK
}

fn run_set(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let mut content = String::new();
    let mut scope = "global".to_string();
    let mut kind = "user".to_string();
    let mut staged = false;

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
        } else if !arg.starts_with('-') && content.is_empty() {
            content = arg.into_owned();
        }
        i += 1;
    }

    if content.is_empty() {
        let _ = writeln!(stderr, "symbrain memory set: content is required");
        return exit::USAGE;
    }

    let store = match open_store(stderr) {
        Ok(s) => s,
        Err(code) => return code,
    };

    match store.set(&content, &scope, &kind, serde_json::Map::new(), staged) {
        Ok(mem) => {
            match format {
                OutputFormat::Json => {
                    let res = serde_json::json!({
                        "id": mem.id,
                        "scope": mem.scope,
                        "kind": mem.kind,
                        "staged": staged,
                    });
                    let _ = writeln!(
                        stdout,
                        "{}",
                        serde_json::to_string_pretty(&res).unwrap_or_default()
                    );
                }
                OutputFormat::Table => {
                    let _ = writeln!(stdout, "Stored memory {}.", mem.id);
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
    let id = match args.first() {
        Some(arg) if !arg.to_string_lossy().starts_with('-') => arg.to_string_lossy().into_owned(),
        _ => {
            let _ = writeln!(stderr, "symbrain memory delete: id is required");
            return exit::USAGE;
        }
    };

    let store = match open_store(stderr) {
        Ok(s) => s,
        Err(code) => return code,
    };

    match store.delete(&id) {
        Ok(true) => {
            match format {
                OutputFormat::Json => {
                    let res = serde_json::json!({
                        "id": id,
                        "deleted": true,
                    });
                    let _ = writeln!(
                        stdout,
                        "{}",
                        serde_json::to_string_pretty(&res).unwrap_or_default()
                    );
                }
                OutputFormat::Table => {
                    let _ = writeln!(stdout, "Deleted memory {id}.");
                }
            }
            exit::OK
        }
        Ok(false) => {
            let _ = writeln!(stderr, "memory {id} not found");
            exit::NOT_FOUND
        }
        Err(err) => {
            let _ = writeln!(stderr, "symbrain memory delete: {err}");
            exit::GENERIC
        }
    }
}

fn run_rules(
    _args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let store = match open_store(stderr) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let memories = match store.list("rules", 100) {
        Ok(m) => m,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain memory rules: {err}");
            return exit::GENERIC;
        }
    };

    match format {
        OutputFormat::Json => {
            let _ = writeln!(
                stdout,
                "{}",
                serde_json::to_string_pretty(&memories).unwrap_or_default()
            );
        }
        OutputFormat::Table => {
            if memories.is_empty() {
                let _ = writeln!(stdout, "No rules found.");
            } else {
                let _ = writeln!(stdout, "ID\tRULE");
                for m in &memories {
                    let _ = writeln!(stdout, "{}\t{}", m.id, m.content);
                }
            }
        }
    }

    exit::OK
}

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
        match open_store(stderr) {
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

fn render_query_log(summary: &serde_json::Value, stdout: &mut dyn Write, format: OutputFormat) {
    match format {
        OutputFormat::Json => {
            let _ = writeln!(
                stdout,
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_default()
            );
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
}
