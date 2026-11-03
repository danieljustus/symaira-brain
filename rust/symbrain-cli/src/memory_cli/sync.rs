//! `memory sync`: `symbrain memory sync` end to end — flag grammar, the
//! argument checks that precede the database and network work, the remote
//! URL guard, then the pull/push/encrypted-relay run (`sync_run`) and the
//! report. Nothing is handed to another implementation.

use super::help::{MEMORY_SYNC_REMOTE_REQUIRED, MEMORY_SYNC_USAGE};
use super::sync_run::{self, Failure, Options, Report};
use super::sync_url;
use super::{OsString, Write, exit, flags, resolve_db_path};
use std::time::{Duration, Instant};
use symbrain_core::config::format_go_quoted;
use symbrain_core::output::OutputFormat;
use symbrain_memory::Store;
use symbrain_memory::sync_client::ClientApplyMode;

const PASSPHRASE_ENV: &str = "SYMBRAIN_MEMORY_SYNC_RELAY_PASSPHRASE";
const TOKEN_ENV: &str = "SYMBRAIN_MEMORY_SYNC_TOKEN";

/// Runs `memory sync`.
#[allow(clippy::too_many_lines)] // One linear pass through the shipped handler.
pub(super) fn run(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let Ok(parsed) = flags::parse(args, "sync", stderr) else {
        return exit::USAGE;
    };
    if let Some(first) = parsed.positional.first() {
        let _ = writeln!(
            stderr,
            "symbrain memory sync: unexpected argument {}",
            format_go_quoted(first)
        );
        return exit::USAGE;
    }
    let remote_bytes = parsed.bytes_of("remote");
    if remote_bytes.is_empty() {
        let _ = write!(stderr, "{MEMORY_SYNC_REMOTE_REQUIRED}{MEMORY_SYNC_USAGE}");
        return exit::USAGE;
    }
    let relay = parsed.flag("encrypted-relay");
    let mut passphrase = parsed.bytes_of("relay-passphrase").to_vec();
    if passphrase.is_empty() {
        passphrase = env_bytes(PASSPHRASE_ENV);
    }
    if relay && passphrase.is_empty() {
        let _ = writeln!(
            stderr,
            "symbrain memory sync: --encrypted-relay requires --relay-passphrase (or ${PASSPHRASE_ENV})"
        );
        return exit::USAGE;
    }
    let remote = flags::go_string(remote_bytes);
    let allow_insecure = parsed.flag("allow-insecure-http");
    if let Err(message) = sync_url::validate_remote(&remote, allow_insecure) {
        let _ = writeln!(stderr, "symbrain memory sync: {message}");
        return exit::USAGE;
    }
    if allow_insecure {
        let _ = writeln!(
            stderr,
            "WARNING: --allow-insecure-http is set: bearer tokens will be sent in the clear to {remote}"
        );
    }
    let (pull, push) = match (parsed.flag("pull"), parsed.flag("push")) {
        (false, false) => (true, true),
        other => other,
    };
    let mut token = parsed.bytes_of("token").to_vec();
    if token.is_empty() {
        token = env_bytes(TOKEN_ENV);
    }
    let timeout_ns = {
        let raw = parsed.bytes_of("timeout");
        if raw.is_empty() {
            60_000_000_000
        } else {
            super::sync_duration::parse_duration(raw).unwrap_or(60_000_000_000)
        }
    };

    let path = resolve_db_path(Some(&parsed.raw("db")));
    let store = match Store::open(&path) {
        Ok(store) => store,
        Err(error) => {
            let _ = writeln!(
                stderr,
                "symbrain memory sync: open memory database: {error}"
            );
            return exit::GENERIC;
        }
    };
    let config = super::config::load();
    let started = Instant::now();
    let deadline = u64::try_from(timeout_ns).map_or(started, |nanos| {
        started
            .checked_add(Duration::from_nanos(nanos))
            .unwrap_or_else(|| started + Duration::from_hours(24 * 365 * 50))
    });
    let options = Options {
        remote: &remote,
        token: &String::from_utf8_lossy(&token),
        pull,
        push,
        relay,
        passphrase: &passphrase,
        deadline,
        mode: ClientApplyMode {
            quantize_binary: config.quantize_binary,
            audit_enabled: config.audit_log_enabled,
        },
    };
    match sync_run::run(&store, &options) {
        Ok(report) => {
            let rendered = match format {
                OutputFormat::Json => format!("{}\n", crate::go_json(&report)),
                OutputFormat::Table => table(&report),
            };
            let _ = stdout.write_all(rendered.as_bytes());
            exit::OK
        }
        Err(Failure::Message(message)) => {
            let _ = writeln!(stderr, "symbrain memory sync: {message}");
            exit::GENERIC
        }
        Err(Failure::NilPanic) => {
            let _ = stderr.write_all(sync_run::nil_panic_text().as_bytes());
            2
        }
    }
}

fn env_bytes(name: &str) -> Vec<u8> {
    flags::bytes(&std::env::var_os(name).unwrap_or_default())
}

fn table(report: &Report) -> String {
    use std::fmt::Write as _;
    let mut text = String::new();
    let mut line = |label: &str, value: &dyn std::fmt::Display| {
        let _ = writeln!(text, "{:<18} {value}", format!("{label}:"));
    };
    line("Remote", &report.remote);
    line("Mode", &report.mode);
    line("Encrypted relay", &report.encrypted_relay);
    if !report.cursor_time.is_zero() {
        line("Cursor", &report.cursor_time.to_rfc3339_utc());
    }
    if !report.server_clock.is_zero() {
        line("Server time", &report.server_clock.to_rfc3339_utc());
    }
    line("Pulled memories", &report.pulled_memories_applied);
    line("Pulled deletes", &report.pulled_deletes_applied);
    line("Pushed memories", &report.pushed_memories);
    line("Pushed deletes", &report.pushed_deletes);
    if report.encrypted_relay {
        line("Relay blobs fetched", &report.relay_blobs_fetched);
        line("Relay blobs stored", &report.relay_blobs_stored);
    }
    text
}

/// Whether Go `time.ParseDuration` accepts `raw` (the flag package reports
/// every failure as `parse error`).
pub(super) fn valid_duration(raw: &[u8]) -> bool {
    super::sync_duration::parse_duration(raw).is_some()
}
