//! Native `memory sync` for the exchanges whose whole behaviour is proven
//! against the shipped client (#762): a plain (non-relay) run against a
//! loopback `http://127.0.0.1:<port>[/prefix]` relay that has nothing to
//! transfer in either direction.
//!
//! Everything else returns `None` and the invocation stays on the shipped
//! implementation: https and non-loopback remotes, the encrypted relay,
//! local changes to push, replies that carry memories, tombstones or a
//! pagination cursor, odd stored cursors, timeouts outside 1 s..=60 s and
//! transport or protocol failures without a proven wording. The only
//! hand-off after network traffic is a read-only `GET`, which the shipped
//! client then repeats; no local state has changed by then.

use super::flags::Args;
use super::sync_decode::{self, Decoded};
use super::sync_http::{self, Fetched, ReadStop};
use super::{Write, exit, resolve_db_path};
use chrono::{DateTime, Timelike, Utc};
use serde::Serialize;
use std::net::Ipv4Addr;
use std::time::Instant;
use symbrain_core::output::OutputFormat;
use symbrain_memory::{Store, StoredCursor, zero_time};

const TOKEN_ENV: &str = "SYMBRAIN_MEMORY_SYNC_TOKEN";
const PAGE_LIMIT: usize = 500;
/// Success replies this native path can accept are tiny; a larger one carries
/// data and is handed off.
const SUCCESS_BODY_LIMIT: usize = 1 << 20;
/// `io.LimitReader(resp.Body, 1<<16)` of the shipped error decoder.
const ERROR_BODY_LIMIT: usize = 1 << 16;

struct Plan {
    remote: String,
    base: String,
    token: Option<String>,
    pull: bool,
    push: bool,
    timeout: std::time::Duration,
    allow_insecure: bool,
}

#[derive(Serialize)]
struct Report<'a> {
    remote: &'a str,
    mode: &'a str,
    encrypted_relay: bool,
    cursor: String,
    server_time: String,
    pulled_memories_applied: u32,
    pulled_deletes_applied: u32,
    pushed_memories: u32,
    pushed_deletes: u32,
    relay_blobs_fetched: u32,
    relay_blobs_stored: u32,
}

/// `None` hands the invocation to the shipped implementation; nothing has been
/// written to `stdout` or `stderr` in that case.
pub(super) fn run(
    parsed: &Args,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> Option<u8> {
    let plan = plan(parsed)?;
    let mut diagnostics = Vec::new();
    if plan.allow_insecure {
        let _ = writeln!(
            diagnostics,
            "WARNING: --allow-insecure-http is set: bearer tokens will be sent in the clear to {}",
            plan.remote
        );
    }
    let path = resolve_db_path(Some(&parsed.raw("db")));
    let store = match Store::open(&path) {
        Ok(store) => store,
        Err(error) => {
            let _ = writeln!(
                diagnostics,
                "symbrain memory sync: open memory database: {error}"
            );
            let _ = stderr.write_all(&diagnostics);
            return Some(exit::GENERIC);
        }
    };
    // The shipped run starts its clock once the database is open.
    let deadline = Instant::now() + plan.timeout;
    let cursor = match store.sync_cursor(&plan.remote).ok()? {
        StoredCursor::Absent => zero_time(),
        StoredCursor::At(time) => time,
        StoredCursor::Unsupported => return None,
    };
    if plan.push && store.has_pending_sync_changes(cursor).ok()? {
        return None;
    }
    let mut server_time = zero_time();
    if plan.pull {
        match pull(&plan, cursor, deadline)? {
            Pulled::Time(time) => server_time = time,
            Pulled::Failed(message) => {
                let _ = writeln!(diagnostics, "symbrain memory sync: {message}");
                let _ = stderr.write_all(&diagnostics);
                return Some(exit::GENERIC);
            }
        }
    }
    // A push-only run reports no relay clock, so it stores the zero cursor.
    store.set_sync_cursor(&plan.remote, server_time).ok()?;
    let _ = stderr.write_all(&diagnostics);
    let mode = match (plan.pull, plan.push) {
        (true, true) => "both",
        (true, false) => "pull",
        _ => "push",
    };
    let report = Report {
        remote: &plan.remote,
        mode,
        encrypted_relay: false,
        cursor: rfc3339_nano(server_time),
        server_time: rfc3339_nano(server_time),
        pulled_memories_applied: 0,
        pulled_deletes_applied: 0,
        pushed_memories: 0,
        pushed_deletes: 0,
        relay_blobs_fetched: 0,
        relay_blobs_stored: 0,
    };
    let rendered = match format {
        OutputFormat::Json => format!("{}\n", crate::go_json(&report)),
        OutputFormat::Table => table(&report, server_time),
    };
    let _ = stdout.write_all(rendered.as_bytes());
    Some(exit::OK)
}

enum Pulled {
    Time(DateTime<Utc>),
    Failed(String),
}

fn pull(plan: &Plan, cursor: DateTime<Utc>, deadline: Instant) -> Option<Pulled> {
    let mut url = format!("{}/api/sync/changes?limit={PAGE_LIMIT}", plan.base);
    if cursor != zero_time() {
        // `url.Values.Encode` escapes the colons of the RFC 3339 form.
        url = format!("{url}&since={}", cursor.format("%Y-%m-%dT%H%%3A%M%%3A%SZ"));
    }
    let failed = |inner: String| Pulled::Failed(format!("pull from {}: {inner}", plan.remote));
    let transport = |cause: &str| failed(format!("request {}: Get \"{url}\": {cause}", plan.base));
    let body_limit = SUCCESS_BODY_LIMIT.max(ERROR_BODY_LIMIT);
    match sync_http::get(&url, plan.token.as_deref(), deadline, body_limit) {
        Fetched::Handoff => None,
        Fetched::Transport(cause) => Some(transport(&cause)),
        Fetched::Reply {
            status,
            body,
            stopped,
        } if status > 300 => {
            let body = &body[..body.len().min(ERROR_BODY_LIMIT)];
            // A stalled or cut body is decoded as far as it was read.
            let _ = stopped;
            let (message, code) = sync_decode::api_error(body)?;
            Some(failed(if code.is_empty() {
                format!("remote returned status {status}: {message}")
            } else {
                format!("remote returned {code} ({status}): {message}")
            }))
        }
        Fetched::Reply { body, stopped, .. } => {
            if let Some(stop) = stopped {
                let cause = match stop {
                    ReadStop::Deadline => "context deadline exceeded",
                    ReadStop::UnexpectedEof => "unexpected EOF",
                };
                return Some(failed(format!("decode remote response: {cause}")));
            }
            if body.len() >= SUCCESS_BODY_LIMIT {
                return None;
            }
            match sync_decode::changes(&body) {
                Decoded::Value(time) => Some(Pulled::Time(time)),
                Decoded::Failure(message) => {
                    Some(failed(format!("decode remote response: {message}")))
                }
                Decoded::Handoff => None,
            }
        }
    }
}

fn plan(parsed: &Args) -> Option<Plan> {
    if parsed.flag("encrypted-relay") {
        return None;
    }
    let remote = std::str::from_utf8(parsed.bytes_of("remote")).ok()?;
    let rest = remote.strip_prefix("http://")?;
    let (authority, path) = rest.find('/').map_or((rest, ""), |at| rest.split_at(at));
    let (host, port) = authority.split_once(':')?;
    let address: Ipv4Addr = host.parse().ok()?;
    if address != Ipv4Addr::LOCALHOST || port.is_empty() || port.starts_with('0') || port.len() > 5
    {
        return None;
    }
    if !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    // Port 80 is left out: the shipped client omits nothing, but the default
    // port is the one place HTTP libraries differ about the Host header.
    if matches!(port.parse::<u16>().ok()?, 80) {
        return None;
    }
    let trimmed = path.trim_end_matches('/');
    if !trimmed.is_empty()
        && !trimmed[1..].split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
    {
        return None;
    }
    let token = {
        let flag = parsed.bytes_of("token");
        let bytes = if flag.is_empty() {
            super::flags::bytes(&std::env::var_os(TOKEN_ENV).unwrap_or_default())
        } else {
            flag.to_vec()
        };
        if bytes.is_empty() {
            None
        } else if bytes.iter().all(|byte| (0x21..=0x7e).contains(byte)) {
            Some(String::from_utf8(bytes).ok()?)
        } else {
            return None;
        }
    };
    let (pull, push) = match (parsed.flag("pull"), parsed.flag("push")) {
        (false, false) => (true, true),
        other => other,
    };
    Some(Plan {
        remote: remote.to_owned(),
        base: remote.trim_end_matches('/').to_owned(),
        token,
        pull,
        push,
        timeout: sync_http::timeout_from_flag(parsed.bytes_of("timeout"))?,
        allow_insecure: parsed.flag("allow-insecure-http"),
    })
}

/// `time.Time` JSON (RFC 3339 with trailing zeros of the fraction trimmed).
fn rfc3339_nano(time: DateTime<Utc>) -> String {
    let base = time.format("%Y-%m-%dT%H:%M:%S").to_string();
    let nanos = time.nanosecond();
    if nanos == 0 {
        return format!("{base}Z");
    }
    format!("{base}.{}Z", format!("{nanos:09}").trim_end_matches('0'))
}

fn table(report: &Report<'_>, server_time: DateTime<Utc>) -> String {
    use std::fmt::Write as _;
    let mut text = String::new();
    let mut line = |label: &str, value: &dyn std::fmt::Display| {
        let _ = writeln!(text, "{:<18} {value}", format!("{label}:"));
    };
    line("Remote", &report.remote);
    line("Mode", &report.mode);
    line("Encrypted relay", &report.encrypted_relay);
    let stamp = server_time.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    if server_time != zero_time() {
        line("Cursor", &stamp);
        line("Server time", &stamp);
    }
    line("Pulled memories", &report.pulled_memories_applied);
    line("Pulled deletes", &report.pulled_deletes_applied);
    line("Pushed memories", &report.pushed_memories);
    line("Pushed deletes", &report.pushed_deletes);
    text
}
