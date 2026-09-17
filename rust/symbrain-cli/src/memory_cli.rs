//! Native `symbrain memory` CLI implementation.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;

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
