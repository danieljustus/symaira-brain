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

mod config;
mod config_float;
mod config_schema;
mod config_value;
mod flag_integer;
mod flag_usage;
mod flags;
mod help;
mod kind;
use help::{
    MEMORY_DELETE_USAGE, MEMORY_DELETE_USAGE_ERROR, MEMORY_LIST_USAGE, MEMORY_RULES_USAGE,
    MEMORY_SEARCH_USAGE, MEMORY_SEARCH_USAGE_ERROR, MEMORY_SET_USAGE, MEMORY_SET_USAGE_ERROR,
    MEMORY_USAGE, QUERY_LOG_USAGE,
};
mod routing;
mod sync;
mod sync_decode;
mod sync_http;
mod sync_native;
pub(crate) use routing::requires_go_fallback;
mod paths;
use paths::{open_store, resolve_db_path};
mod read;
use read::{run_list, table_content};
mod search;
use search::run_search;
mod write;
mod write_output;
use write::{run_delete, run_set};
mod rules;
use rules::run_rules;
mod query_log;
use query_log::run_query_log;

/// Runs `symbrain memory`.
pub fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    run_with_stdout(args, stdout, stderr, format, false)
}

/// Whether `args` are a `memory sync` invocation.
pub(crate) fn is_sync(args: &[OsString]) -> bool {
    args.first().is_some_and(|verb| verb == "sync")
}

/// Runs `memory sync`; `None` leaves the invocation to the shipped
/// implementation.
pub(crate) fn run_sync(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> Option<u8> {
    sync::run(&args[1..], stdout, stderr, format)
}

pub(super) fn run_with_stdout(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
    process_stdout: bool,
) -> u8 {
    if args.is_empty() {
        let _ = write!(stderr, "{MEMORY_USAGE}");
        return exit::USAGE;
    }

    let verb = args[0].to_string_lossy();
    let rest = &args[1..];

    match verb.as_ref() {
        "-h" | "--help" => {
            let _ = write!(stdout, "{MEMORY_USAGE}");
            exit::OK
        }
        "list" => run_list(rest, stdout, stderr, format),
        "search" => run_search(rest, stdout, stderr, format),
        "set" => run_set(rest, stdout, stderr, format, process_stdout),
        "delete" => run_delete(rest, stdout, stderr, format),
        "rules" => run_rules(rest, stdout, stderr, format),
        "query-log" => run_query_log(rest, stdout, stderr, format),
        // Reached only through direct calls: the CLI routes `sync` through
        // `run_sync`, which can hand an invocation to the shipped
        // implementation. Fail closed instead of synchronising here.
        "sync" => sync::run(rest, stdout, stderr, format).unwrap_or(exit::GENERIC),
        _ => {
            let _ = writeln!(
                stderr,
                "symbrain memory: unknown subcommand {}\n",
                symbrain_core::config::format_go_quoted(&args[0])
            );
            let _ = write!(stderr, "{MEMORY_USAGE}");
            exit::USAGE
        }
    }
}

#[cfg(test)]
mod sync_tests;
#[cfg(test)]
mod tests;
