//! Native implementation of `symbrain guard grants`.

use std::env;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};

use symbrain_core::exit;
use symbrain_guard_core::go_json::format_go_rfc3339;
use symbrain_guard_core::go_time;

#[path = "guard_grants_store.rs"]
mod store;
use store::Store;

const USAGE: &str = "Usage:\n  symguard grants list\n  symguard grants revoke <id> | --all\n\nCommands:\n  list      List active grants\n  revoke    Revoke a grant by ID, or revoke every grant with --all\n\nRun 'symguard grants <command> --help' for details.\n";
const ZERO_TIME: &str = "0001-01-01T00:00:00Z";

/// Runs the grants subcommand. Grant command errors are printed on stdout to
/// preserve the historical Go command's stream and exit-code contract.
pub fn run(args: &[OsString], stdout: &mut dyn Write, _stderr: &mut dyn Write) -> u8 {
    let dir = default_dir();
    run_at_dir(args, &dir, stdout)
}

/// Runs grants against an explicit data directory; useful for isolated tests.
pub fn run_at_dir(args: &[OsString], dir: &Path, stdout: &mut dyn Write) -> u8 {
    let Some(command) = args.first().map(|argument| argument.to_string_lossy()) else {
        write_usage(stdout);
        return exit::OK;
    };
    match command.as_ref() {
        "list" => list(dir, stdout),
        "revoke" => revoke(&args[1..], dir, stdout),
        "help" | "--help" | "-h" => {
            write_usage(stdout);
            exit::OK
        }
        other => {
            let _ = writeln!(stdout, "unknown grants subcommand: {other}\n");
            write_usage(stdout);
            exit::OK
        }
    }
}

fn list(dir: &Path, stdout: &mut dyn Write) -> u8 {
    let store = match Store::open(dir) {
        Ok(store) => store,
        Err(error) => {
            let _ = writeln!(stdout, "grants: {error}");
            return exit::OK;
        }
    };
    let active = store.active();
    if active.is_empty() {
        let _ = writeln!(stdout, "No active grants.");
        return exit::OK;
    }

    let rows: Vec<[String; 5]> = active
        .iter()
        .map(|grant| {
            [
                grant.id.clone(),
                grant.scope.clone(),
                grant.subject.clone(),
                format!("{}@{}", grant.origin.via, grant.origin.epoch),
                display_time(&grant.granted_at),
            ]
        })
        .collect();
    let headers = [
        "ID".to_owned(),
        "SCOPE".to_owned(),
        "SUBJECT".to_owned(),
        "ORIGIN".to_owned(),
        "GRANTED_AT".to_owned(),
    ];
    let mut widths = headers.each_ref().map(String::len);
    for row in &rows {
        for (index, value) in row.iter().enumerate() {
            widths[index] = widths[index].max(value.len());
        }
    }
    let mut output = String::new();
    write_row(&mut output, &headers, &widths);
    for row in &rows {
        write_row(&mut output, row, &widths);
    }
    let _ = stdout.write_all(output.as_bytes());
    exit::OK
}

fn revoke(args: &[OsString], dir: &Path, stdout: &mut dyn Write) -> u8 {
    let mut all = false;
    let mut id = None;
    for argument in args {
        let argument = argument.to_string_lossy();
        match argument.as_ref() {
            "--all" => all = true,
            "--help" | "-h" => {
                write_usage(stdout);
                return exit::OK;
            }
            value if value.starts_with('-') => {
                let _ = writeln!(stdout, "grants revoke: unexpected flag {value:?}");
                write_usage(stdout);
                return exit::OK;
            }
            "" => {}
            value if id.is_none() && !value.is_empty() => id = Some(value.to_owned()),
            value => {
                let _ = writeln!(stdout, "grants revoke: unexpected argument {value:?}");
                write_usage(stdout);
                return exit::OK;
            }
        }
    }
    if all && id.is_some() {
        let _ = writeln!(
            stdout,
            "grants revoke: --all cannot be combined with a grant ID"
        );
        write_usage(stdout);
        return exit::OK;
    }
    let mut store = match Store::open(dir) {
        Ok(store) => store,
        Err(error) => {
            let _ = writeln!(stdout, "grants: {error}");
            return exit::OK;
        }
    };
    if all {
        match store.revoke_all() {
            Ok(count) => {
                let _ = writeln!(stdout, "Revoked {count} grant(s).");
            }
            Err(error) => {
                let _ = writeln!(stdout, "grants: {error}");
            }
        }
        return exit::OK;
    }
    let Some(id) = id else {
        let _ = writeln!(stdout, "grants revoke: missing grant ID or --all");
        write_usage(stdout);
        return exit::OK;
    };
    match store.revoke(&id) {
        Ok(()) => {
            let _ = writeln!(stdout, "Revoked grant {id}.");
        }
        Err(error) => {
            let _ = writeln!(stdout, "grants: {error}");
        }
    }
    exit::OK
}

fn write_usage(stdout: &mut dyn Write) {
    let _ = stdout.write_all(USAGE.as_bytes());
}

fn display_time(value: &str) -> String {
    if value.is_empty() {
        ZERO_TIME.to_owned()
    } else {
        go_time::parse(value.as_bytes()).map_or_else(|_| value.to_owned(), format_go_rfc3339)
    }
}

fn write_row(output: &mut String, row: &[String; 5], widths: &[usize; 5]) {
    for index in 0..row.len() {
        if index > 0 {
            output.push_str("  ");
        }
        if index + 1 == row.len() {
            output.push_str(&row[index]);
        } else {
            let _ = write!(output, "{:<width$}", row[index], width = widths[index]);
        }
    }
    output.push('\n');
}

fn default_dir() -> PathBuf {
    if let Some(path) = env::var_os("SYMGUARD_DATA").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    if let Some(path) = env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(path).join("symguard");
    }
    let home_key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    env::var_os(home_key)
        .filter(|value| !value.is_empty())
        .map_or_else(
            || env::temp_dir().join("symguard"),
            |home| PathBuf::from(home).join(".local/share/symguard"),
        )
}
