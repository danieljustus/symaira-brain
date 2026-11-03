//! Daemon-less and client-side commands: `session id`, `cache`, `trace`,
//! `watch`, `flow run` and `doctor`.

use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use symbrowse_core::{
    budget, cache,
    config::{Config, FlagOverrides, LoadContext, load},
    error::ErrorCode,
    flows,
    go_value::{go_struct, raw_scalar},
    journal, runner, trace,
};
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};

use crate::app::{CliError, Ctx, Handled, Outcome, daemon_request, response_error};

fn go_quote(value: &str) -> String {
    symbrowse_core::go_quote::quote(value)
}

fn done(result: Handled) -> Outcome {
    match result {
        Ok(()) => Outcome::Done,
        Err(error) => Outcome::Fail(error),
    }
}

pub fn dispatch(ctx: &mut Ctx<'_>, key: &str, args: &[String]) -> Outcome {
    match key {
        "session id" => done(session_id(ctx)),
        "cache get" => done(cache_get(ctx, args)),
        "cache list" => done(cache_list(ctx)),
        "cache clear" => done(cache_clear(ctx)),
        "trace export" => done(trace_export(ctx)),
        "trace replay" => done(trace_replay(ctx, args)),
        "watch" => watch(ctx),
        "flow run" => done(flow_run(ctx, args)),
        "doctor" => doctor(ctx),
        "read" => done(crate::read::read(ctx, args)),
        "flow list" => done(flow_list(ctx)),
        "flow validate" => done(flow_validate(ctx, args)),
        other => Outcome::Fail(CliError::plain(format!(
            "unknown command {}",
            go_quote(other)
        ))),
    }
}

/// `err.Error()` of an `*os.PathError` cause (the lower-cased errno text).
pub fn go_errno(error: &std::io::Error) -> String {
    let text = error.to_string();
    let text = text.split(" (os error").next().unwrap_or(&text);
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_lowercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Time helpers (Go `time.Time` rendering)

/// `time.RFC3339Nano` with Go's trailing-zero trimming.
pub fn go_rfc3339(moment: OffsetDateTime) -> String {
    let nanos = moment.nanosecond();
    let mut out = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        moment.year(),
        u8::from(moment.month()),
        moment.day(),
        moment.hour(),
        moment.minute(),
        moment.second()
    );
    if nanos != 0 {
        let fraction = format!("{nanos:09}");
        out.push('.');
        out.push_str(fraction.trim_end_matches('0'));
    }
    let offset = moment.offset();
    if offset.is_utc() {
        out.push('Z');
    } else {
        let total = offset.whole_seconds();
        let sign = if total < 0 { '-' } else { '+' };
        let total = total.abs();
        out.push_str(&format!(
            "{sign}{:02}:{:02}",
            total / 3600,
            (total % 3600) / 60
        ));
    }
    out
}

const ZERO_TIME: &str = "0001-01-01T00:00:00Z";

fn parse_time(text: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(text, &Rfc3339).ok()
}

fn is_zero_time(text: &str) -> bool {
    parse_time(text).is_none_or(|moment| moment.year() <= 1)
}

/// `time.Duration.String()` for whole seconds.
pub fn go_duration(seconds: i64) -> String {
    if seconds == 0 {
        return "0s".into();
    }
    let sign = if seconds < 0 { "-" } else { "" };
    let total = seconds.abs();
    let (hours, minutes, secs) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{sign}{hours}h{minutes}m{secs}s")
    } else if minutes > 0 {
        format!("{sign}{minutes}m{secs}s")
    } else {
        format!("{sign}{secs}s")
    }
}

fn now() -> OffsetDateTime {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    OffsetDateTime::from_unix_timestamp_nanos(elapsed.as_nanos() as i128)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

// ---------------------------------------------------------------------------
// config.Load() (no command-line overrides)

fn load_config() -> Result<Config, CliError> {
    let context = LoadContext::from_process(FlagOverrides::default())
        .map_err(|error| CliError::plain(error.to_string()))?;
    load(&context)
        .map(|result| result.config)
        .map_err(|error| CliError::plain(error.to_string()))
}

// ---------------------------------------------------------------------------
// session id

fn getwd() -> Result<PathBuf, CliError> {
    let physical = std::env::current_dir().map_err(|error| {
        CliError::plain(format!("determine working directory: {}", go_errno(&error)))
    })?;
    if let Some(pwd) = std::env::var_os("PWD").map(PathBuf::from)
        && pwd.is_absolute()
        && let (Ok(a), Ok(b)) = (fs::metadata(&pwd), fs::metadata("."))
        && a.dev() == b.dev()
        && a.ino() == b.ino()
    {
        return Ok(pwd);
    }
    Ok(physical)
}

/// Lexical `filepath.Clean` for absolute paths.
fn clean(path: &Path) -> PathBuf {
    let mut parts: Vec<String> = Vec::new();
    for component in path.to_string_lossy().split('/') {
        match component {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other.to_owned()),
        }
    }
    PathBuf::from(format!("/{}", parts.join("/")))
}

fn git_output(dir: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn short_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn session_id(ctx: &mut Ctx<'_>) -> Handled {
    let scope = ctx.flags.string("scope", "worktree");
    let prefix = ctx.flags.string("prefix", "");
    if !matches!(scope.as_str(), "worktree" | "repo" | "cwd") {
        return Err(CliError::plain(format!(
            "invalid scope {}",
            go_quote(&scope)
        )));
    }
    let dir = clean(&getwd()?);
    let apply = |hash: String| {
        if prefix.is_empty() {
            hash
        } else {
            format!("{prefix}-{hash}")
        }
    };
    let (id, origin, fallback) = if scope == "cwd" {
        (
            apply(short_hash(&dir.to_string_lossy())),
            dir.to_string_lossy().into_owned(),
            true,
        )
    } else {
        let layout = git_output(&dir, &["rev-parse", "--show-toplevel"]).map(|top| {
            let worktree = clean(Path::new(&top));
            let repo = match git_output(&dir, &["rev-parse", "--git-common-dir"]) {
                Some(common) => {
                    let common = if Path::new(&common).is_absolute() {
                        PathBuf::from(common)
                    } else {
                        worktree.join(common)
                    };
                    let common = clean(&common);
                    if common.file_name().is_some_and(|name| name == ".git") {
                        common.parent().map_or(common.clone(), Path::to_path_buf)
                    } else {
                        common
                    }
                }
                None => worktree.clone(),
            };
            (repo, worktree)
        });
        match layout {
            None => (
                apply(short_hash(&dir.to_string_lossy())),
                dir.to_string_lossy().into_owned(),
                true,
            ),
            Some((repo, worktree)) => {
                let origin = if scope == "repo" {
                    repo
                } else if worktree.as_os_str().is_empty() {
                    repo
                } else {
                    worktree
                };
                let origin = origin.to_string_lossy().into_owned();
                (apply(short_hash(&origin)), origin, false)
            }
        }
    };
    if ctx.structured() {
        let mut fields = vec![
            ("id", "id", Value::String(id.clone())),
            ("scope", "scope", Value::String(scope)),
            (
                if prefix.is_empty() { "" } else { "prefix" },
                "prefix",
                Value::String(prefix),
            ),
            ("origin_path", "originpath", Value::String(origin)),
            (
                if fallback { "fallback" } else { "" },
                "fallback",
                Value::Bool(fallback),
            ),
        ];
        fields.retain(|_| true);
        return ctx.write_ok(go_struct(fields));
    }
    ctx.println(&id);
    Ok(())
}

// ---------------------------------------------------------------------------
// cache

struct ListEntry {
    id: String,
    kind: &'static str,
    bytes: u64,
    created: Option<OffsetDateTime>,
    created_raw: String,
    expires: String,
    expired: bool,
}

fn output_cache(config: &Config) -> cache::Cache {
    cache::Cache::new(
        Path::new(&config.cache_dir).join("out"),
        Duration::from_secs(u64::try_from(config.cache_ttl_hours).unwrap_or(0) * 3600),
    )
}

fn fetch_dir(config: &Config) -> PathBuf {
    Path::new(&config.cache_dir).join("fetch")
}

fn fetch_ttl(config: &Config) -> Option<Duration> {
    let hours = u64::try_from(config.cache_ttl_hours).unwrap_or(0);
    (hours > 0).then(|| Duration::from_secs(hours * 3600))
}

fn ensure_fetch_dir(dir: &Path) {
    if fs::create_dir_all(dir).is_ok() {
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
}

struct FetchEntry {
    key: String,
    bytes: u64,
    stored_at: OffsetDateTime,
}

fn fetch_entries(config: &Config) -> Vec<(FetchEntry, bool)> {
    let dir = fetch_dir(config);
    ensure_fetch_dir(&dir);
    let ttl = fetch_ttl(config);
    let mut result = Vec::new();
    let Ok(shards) = fs::read_dir(&dir) else {
        return result;
    };
    for shard in shards.flatten() {
        let Ok(files) = fs::read_dir(shard.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(key) = name.strip_suffix(".meta.json") else {
                continue;
            };
            let Ok(raw) = fs::read(&path) else { continue };
            let Ok(meta) = serde_json::from_slice::<Value>(&raw) else {
                continue;
            };
            let Some(stored_at) = meta
                .get("stored_at")
                .and_then(Value::as_str)
                .and_then(parse_time)
            else {
                continue;
            };
            let body = path.with_file_name(format!("{key}.body"));
            let Ok(body_info) = fs::metadata(&body) else {
                continue;
            };
            let expired = ttl.is_some_and(|ttl| {
                now() > stored_at + time::Duration::seconds(ttl.as_secs() as i64)
            });
            if expired {
                let _ = fs::remove_file(&body);
                let _ = fs::remove_file(&path);
                continue;
            }
            result.push((
                FetchEntry {
                    key: key.to_owned(),
                    bytes: body_info.len() + raw.len() as u64,
                    stored_at,
                },
                false,
            ));
        }
    }
    result
}

fn all_cache_entries(config: &Config) -> Result<Vec<ListEntry>, CliError> {
    let mut entries = Vec::new();
    let listed = output_cache(config)
        .list()
        .map_err(|error| CliError::plain(error.to_string()))?;
    for entry in listed {
        let created = parse_time(&entry.created_at);
        entries.push(ListEntry {
            id: entry.id,
            kind: "output",
            bytes: entry.bytes,
            created_raw: created.map_or(entry.created_at.clone(), go_rfc3339),
            created,
            expires: if is_zero_time(&entry.expires_at) {
                ZERO_TIME.to_owned()
            } else {
                parse_time(&entry.expires_at).map_or(entry.expires_at.clone(), go_rfc3339)
            },
            expired: entry.expired,
        });
    }
    let ttl = fetch_ttl(config);
    for (entry, expired) in fetch_entries(config) {
        entries.push(ListEntry {
            id: format!("fetch:{}", entry.key),
            kind: "fetch-response",
            bytes: entry.bytes,
            created_raw: go_rfc3339(entry.stored_at),
            created: Some(entry.stored_at),
            expires: ttl.map_or_else(
                || ZERO_TIME.to_owned(),
                |ttl| go_rfc3339(entry.stored_at + time::Duration::seconds(ttl.as_secs() as i64)),
            ),
            expired,
        });
    }
    entries.sort_by(|a, b| match (a.created, b.created) {
        (Some(x), Some(y)) if x != y => x.cmp(&y),
        _ => a.id.cmp(&b.id),
    });
    Ok(entries)
}

fn valid_fetch_key(key: &str) -> bool {
    key.len() == 64
        && key
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn load_cache_content(config: &Config, id: &str) -> Result<Vec<u8>, CliError> {
    if let Some(key) = id.strip_prefix("fetch:") {
        let not_found = || CliError::plain(format!("fetch cache entry not found: {key}"));
        if !valid_fetch_key(key) {
            return Err(not_found());
        }
        let dir = fetch_dir(config);
        ensure_fetch_dir(&dir);
        let meta_path = dir.join(&key[..2]).join(format!("{key}.meta.json"));
        let raw = fs::read(&meta_path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                not_found()
            } else {
                CliError::plain(format!(
                    "open {}: {}",
                    meta_path.display(),
                    go_errno(&error)
                ))
            }
        })?;
        let meta: Value =
            serde_json::from_slice(&raw).map_err(|error| CliError::plain(error.to_string()))?;
        let stored = meta
            .get("stored_at")
            .and_then(Value::as_str)
            .and_then(parse_time);
        let meta_ttl = meta.get("ttl").and_then(Value::as_i64).unwrap_or(0);
        let ttl = if meta_ttl > 0 {
            Some(Duration::from_nanos(meta_ttl as u64))
        } else {
            fetch_ttl(config)
        };
        if let (Some(ttl), Some(stored)) = (ttl, stored)
            && now() - stored > time::Duration::seconds(ttl.as_secs() as i64)
        {
            return Err(CliError::plain(format!("fetch cache entry expired: {key}")));
        }
        return fs::read(dir.join(&key[..2]).join(format!("{key}.body"))).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                not_found()
            } else {
                CliError::plain(go_errno(&error))
            }
        });
    }
    output_cache(config)
        .load(id)
        .map_err(|error| CliError::plain(error.to_string()))
}

fn parse_range(spec: &str) -> Result<(usize, usize), CliError> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Ok((0, 0));
    }
    let (first, second) = match spec.split_once('-') {
        Some((a, b)) => (a, Some(b)),
        None => (spec, None),
    };
    let atoi = |text: &str| {
        text.parse::<i64>().ok().filter(|_| {
            !text.is_empty()
                && text
                    .bytes()
                    .skip(usize::from(text.starts_with(['+', '-'])))
                    .all(|b| b.is_ascii_digit())
        })
    };
    let mut start = 0i64;
    let mut end = 0i64;
    if !first.is_empty() {
        match atoi(first) {
            Some(value) if value >= 1 => start = value,
            _ => {
                return Err(CliError::plain(format!(
                    "invalid range {}: start must be a positive line number",
                    go_quote(spec)
                )));
            }
        }
    }
    if let Some(second) = second.filter(|s| !s.is_empty()) {
        match atoi(second) {
            Some(value) if value >= start => end = value,
            _ => {
                return Err(CliError::plain(format!(
                    "invalid range {}: end must be >= start",
                    go_quote(spec)
                )));
            }
        }
    }
    Ok((start as usize, end as usize))
}

fn cache_get(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let config = load_config()?;
    let content = load_cache_content(&config, &args[0])?;
    let text = String::from_utf8_lossy(&content).into_owned();
    let range = ctx.flags.string("range", "");
    if range.is_empty() {
        if ctx.structured() {
            return ctx.write_ok(json!({"cache_id": args[0], "content": text}));
        }
        ctx.println(&text);
        return Ok(());
    }
    let (start, end) = parse_range(&range)?;
    let lines = budget::line_range(&text, start, end);
    if ctx.structured() {
        return ctx.write_ok(json!({"cache_id": args[0], "range": range, "content": lines}));
    }
    ctx.println(&lines);
    Ok(())
}

fn cache_list(ctx: &mut Ctx<'_>) -> Handled {
    let config = load_config()?;
    let entries = all_cache_entries(&config)?;
    if ctx.structured() {
        let list: Vec<Value> = entries
            .iter()
            .map(|entry| {
                go_struct(vec![
                    ("id", "id", Value::String(entry.id.clone())),
                    ("kind", "kind", Value::String(entry.kind.to_owned())),
                    ("bytes", "bytes", Value::from(entry.bytes)),
                    ("created_at", "createdat", raw_scalar(&entry.created_raw)),
                    ("expires_at", "expiresat", raw_scalar(&entry.expires)),
                    ("expired", "expired", Value::Bool(entry.expired)),
                ])
            })
            .collect();
        return ctx.write_ok(json!({"entries": list}));
    }
    if entries.is_empty() {
        ctx.println("cache is empty");
        return Ok(());
    }
    let current = now();
    for entry in &entries {
        let age = entry
            .created
            .map_or(0, |created| (current - created).whole_seconds());
        let expiry = match parse_time(&entry.expires) {
            Some(expires) if expires.year() > 1 => {
                format!("{} left", go_duration((expires - current).whole_seconds()))
            }
            _ => "never".to_owned(),
        };
        let line = format!(
            "{}\t{}\t{} bytes\t{} old\t{}",
            entry.id,
            entry.kind,
            entry.bytes,
            go_duration(age),
            expiry
        );
        ctx.println(&line);
    }
    Ok(())
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "y" } else { "ies" }
}

fn cache_clear(ctx: &mut Ctx<'_>) -> Handled {
    let config = load_config()?;
    let before = all_cache_entries(&config)?.len();
    output_cache(&config)
        .clear()
        .map_err(|error| CliError::plain(error.to_string()))?;
    clear_fetch(&config)?;
    let remaining = all_cache_entries(&config)?.len();
    if remaining != 0 {
        return Err(CliError::plain(format!(
            "cache clear left {remaining} entr{}",
            plural(remaining)
        )));
    }
    if ctx.structured() {
        return ctx.write_ok(json!({"cleared": before}));
    }
    let line = format!("cleared {before} cache entr{}", plural(before));
    ctx.println(&line);
    Ok(())
}

fn clear_fetch(config: &Config) -> Result<(), CliError> {
    let dir = fetch_dir(config);
    ensure_fetch_dir(&dir);
    let Ok(shards) = fs::read_dir(&dir) else {
        return Ok(());
    };
    for shard in shards.flatten() {
        let Ok(files) = fs::read_dir(shard.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            let name = path.to_string_lossy();
            if name.ends_with(".body") || name.ends_with(".meta.json") || name.ends_with(".tmp") {
                fs::remove_file(&path).map_err(|error| {
                    CliError::plain(format!("remove {name}: {}", go_errno(&error)))
                })?;
            }
        }
    }
    let _ = fs::remove_file(dir.join("cache-index.json"));
    Ok(())
}

// ---------------------------------------------------------------------------
// trace

fn session_flag(ctx: &Ctx<'_>) -> String {
    ctx.flags.string("session", "default")
}

fn io_path_error(op: &str, path: &str, error: &std::io::Error) -> String {
    format!("{op} {path}: {}", go_errno(error))
}

fn trace_export(ctx: &mut Ctx<'_>) -> Handled {
    let session = session_flag(ctx);
    let response = daemon_request(
        &session,
        "journal.show",
        Some(json!({"session": session})),
        None,
    )?;
    if !response.success {
        return Err(response_error(&response));
    }
    let entries = response
        .data
        .as_ref()
        .and_then(|data| data.get("entries"))
        .and_then(Value::as_array)
        .map(|items| items.iter().map(journal_entry).collect::<Vec<_>>())
        .unwrap_or_default();
    let created = go_rfc3339(now().to_offset(UtcOffset::UTC));
    let file = trace::export(&entries, session.clone(), created);
    if file.steps.is_empty() {
        return Err(CliError::plain(format!(
            "no replayable steps in the journal of session {}",
            go_quote(&session)
        )));
    }
    let out = ctx.flags.string("out", "trace.json");
    let steps: Vec<Value> = file.steps.iter().map(trace_step_value).collect();
    let document = go_struct(vec![
        (
            "schema_version",
            "schemaversion",
            Value::from(file.schema_version),
        ),
        (
            "created_at",
            "createdat",
            Value::String(file.created_at.clone()),
        ),
        ("session", "session", Value::String(file.session.clone())),
        ("steps", "steps", Value::Array(steps)),
    ]);
    let raw = symbrowse_core::go_value::marshal_indent(&document);
    let mut handle = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&out)
        .map_err(|error| {
            CliError::plain(format!(
                "write trace: {}",
                io_path_error("open", &out, &error)
            ))
        })?;
    handle.write_all(raw.as_bytes()).map_err(|error| {
        CliError::plain(format!(
            "write trace: {}",
            io_path_error("write", &out, &error)
        ))
    })?;
    if ctx.structured() {
        return ctx.write_ok(json!({"steps": file.steps.len(), "file": out}));
    }
    let line = format!("exported {} step(s) to {out}", file.steps.len());
    ctx.println(&line);
    Ok(())
}

fn journal_entry(value: &Value) -> journal::Entry {
    let fields = value.as_object().cloned().unwrap_or_default();
    let text = |key: &str| {
        fields
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    journal::Entry {
        schema_version: fields
            .get("schema_version")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        timestamp: text("timestamp"),
        session: text("session"),
        command: text("command"),
        ref_key: text("ref_key"),
        url_before: text("url_before"),
        url_after: text("url_after"),
        args: fields.get("args").cloned().filter(|v| !v.is_null()),
        result: text("result"),
        reason: text("reason"),
        risk_class: text("risk_class"),
        decider: text("decider"),
        duration_ms: fields.get("duration_ms").and_then(Value::as_u64),
    }
}

fn trace_step_value(step: &trace::Step) -> Value {
    let pairs = [
        ("command", &step.command),
        ("selector", &step.selector),
        ("value", &step.value),
        ("key", &step.key),
        ("url", &step.url),
        ("expected_url", &step.expected_url),
    ];
    go_struct(
        pairs
            .into_iter()
            .filter(|(name, value)| *name == "command" || !value.is_empty())
            .map(|(name, value)| (name, name, Value::String(value.clone())))
            .collect(),
    )
}

fn trace_replay(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let path = &args[0];
    let raw = fs::read(path).map_err(|error| {
        CliError::plain(format!(
            "read trace: {}",
            io_path_error("open", path, &error)
        ))
    })?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let value: Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_) => {
            let detail = crate::gojson::scan(&text)
                .err()
                .unwrap_or_else(|| "unexpected end of JSON input".to_owned());
            return Err(CliError::plain(format!("parse trace: {detail}")));
        }
    };
    let version = value
        .get("schema_version")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    if version as i64 != trace::SCHEMA_VERSION {
        return Err(CliError::plain(format!(
            "unsupported trace schema version {}",
            version as i64
        )));
    }
    let steps: Vec<Value> = value
        .get("steps")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    let fields = item.as_object().cloned().unwrap_or_default();
                    let get = |key: &str| {
                        fields
                            .get(key)
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    };
                    trace_step_value(&trace::Step {
                        command: get("command"),
                        selector: get("selector"),
                        value: get("value"),
                        key: get("key"),
                        url: get("url"),
                        expected_url: get("expected_url"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let session = session_flag(ctx);
    let response = daemon_request(
        &session,
        "trace.replay",
        Some(json!({"steps": steps})),
        None,
    )?;
    if ctx.structured() {
        return ctx.write_response(&response);
    }
    if !response.success {
        return Err(response_error(&response));
    }
    let data = response
        .data
        .as_ref()
        .map_or(Value::Null, symbrowse_core::go_value::to_go_any);
    let text = symbrowse_core::go_value::marshal_indent(&data);
    ctx.println(&text);
    Ok(())
}

// ---------------------------------------------------------------------------
// watch

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

fn install_signal_handlers() -> Option<tokio::runtime::Runtime> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .ok()?;
    runtime.spawn(async {
        use tokio::signal::unix::{SignalKind, signal};
        if let (Ok(mut term), Ok(mut int)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
        ) {
            tokio::select! {
                _ = term.recv() => {}
                _ = int.recv() => {}
            }
            INTERRUPTED.store(true, Ordering::SeqCst);
        }
    });
    Some(runtime)
}

fn watch(ctx: &mut Ctx<'_>) -> Outcome {
    let session = ctx.flags.string("session", "default");
    if ctx.flags.bool("take-over") {
        let reason = ctx.flags.string("reason", "");
        if reason.is_empty() {
            return Outcome::Fail(CliError::invalid_args(
                "watch --take-over requires --reason",
            ));
        }
        return done(watch_take_over(ctx, &session, &reason));
    }
    if ctx.captured {
        return Outcome::Fail(CliError::plain("watch cannot run inside batch"));
    }
    let _runtime = install_signal_handlers();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let _ = writeln!(
        out,
        "watching session {} (read-only; Ctrl-C to stop)",
        go_quote(&session)
    );
    let _ = out.flush();
    let mut seen = 0usize;
    loop {
        if INTERRUPTED.load(Ordering::SeqCst) {
            return Outcome::Raw(ExitCode::SUCCESS);
        }
        match daemon_request(&session, "journal.show", None, None) {
            Err(_) => {
                if sleep_interruptible(Duration::from_secs(1)) {
                    return Outcome::Raw(ExitCode::SUCCESS);
                }
                continue;
            }
            Ok(response) => {
                if !response.success {
                    return Outcome::Fail(response_error(&response));
                }
                let entries = response
                    .data
                    .as_ref()
                    .and_then(|data| data.get("entries"))
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for entry in entries.iter().skip(seen) {
                    let fields = entry.as_object().cloned().unwrap_or_default();
                    let text = |key: &str| {
                        fields
                            .get(key)
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    };
                    let _ = writeln!(
                        out,
                        "{}\t{}\tclass={}\tdecider={}\t{}",
                        text("timestamp"),
                        text("command"),
                        text("risk_class"),
                        text("decider"),
                        text("result")
                    );
                }
                let _ = out.flush();
                seen = entries.len();
            }
        }
        if sleep_interruptible(Duration::from_millis(500)) {
            return Outcome::Raw(ExitCode::SUCCESS);
        }
    }
}

fn sleep_interruptible(total: Duration) -> bool {
    let step = Duration::from_millis(25);
    let mut waited = Duration::ZERO;
    while waited < total {
        if INTERRUPTED.load(Ordering::SeqCst) {
            return true;
        }
        std::thread::sleep(step);
        waited += step;
    }
    INTERRUPTED.load(Ordering::SeqCst)
}

fn watch_take_over(ctx: &mut Ctx<'_>, session: &str, reason: &str) -> Handled {
    let response = daemon_request(
        session,
        "handoff",
        Some(json!({"reason": reason, "timeout": "5m"})),
        None,
    )?;
    if ctx.structured() {
        return ctx.write_response(&response);
    }
    if !response.success {
        return Err(response_error(&response));
    }
    let data = response
        .data
        .as_ref()
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let status = data
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let prompt = data
        .get("prompt_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !status.is_empty() {
        let line = if prompt.is_empty() {
            format!("handoff {status}")
        } else {
            format!("handoff {status} (prompt {prompt})")
        };
        ctx.println(&line);
        return Ok(());
    }
    let pretty = symbrowse_core::go_value::marshal_indent(&symbrowse_core::go_value::to_go_any(
        response.data.as_ref().unwrap_or(&Value::Null),
    ));
    ctx.println(&pretty);
    Ok(())
}

// ---------------------------------------------------------------------------
// flow run

fn flow_file(name: &str) -> Result<String, CliError> {
    let ext = Path::new(name).extension().and_then(|e| e.to_str());
    if matches!(ext, Some("yaml" | "yml")) && fs::metadata(name).is_ok() {
        return Ok(name.to_owned());
    }
    let mut candidates = vec![
        format!(".symbrowse/flows/{name}.yaml"),
        format!(".symbrowse/flows/{name}.yml"),
    ];
    if let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
        let global = Path::new(&home).join(".config/symbrowse/flows");
        candidates.push(
            global
                .join(format!("{name}.yaml"))
                .to_string_lossy()
                .into_owned(),
        );
        candidates.push(
            global
                .join(format!("{name}.yml"))
                .to_string_lossy()
                .into_owned(),
        );
    }
    candidates
        .into_iter()
        .find(|candidate| fs::metadata(candidate).is_ok())
        .ok_or_else(|| {
            CliError::plain(format!(
                "flow {} not found (looked in ./.symbrowse/flows and ~/.config/symbrowse/flows)",
                go_quote(name)
            ))
        })
}

fn flow_run(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let path = flow_file(&args[0])?;
    let data = fs::read(&path).map_err(|error| {
        CliError::plain(format!(
            "read flow {}: {}",
            go_quote(&path),
            io_path_error("open", &path, &error)
        ))
    })?;
    let flow =
        flows::parse(&data, path.clone()).map_err(|error| CliError::plain(error.to_string()))?;
    let mut inputs = std::collections::BTreeMap::new();
    for entry in ctx.flags.array("input") {
        match entry.split_once('=') {
            Some((key, value)) if !key.trim().is_empty() => {
                inputs.insert(key.trim().to_owned(), value.to_owned());
            }
            _ => {
                return Err(CliError::plain(format!(
                    "invalid --input {} (expected k=v)",
                    go_quote(&entry)
                )));
            }
        }
    }
    let dry = ctx.flags.bool("dry-run");
    let session = ctx.flags.string("session", "default");
    let socket_error = "socket path is required";
    let mut executor = |_: &str, _: Value| -> Result<Value, runner::ExecutionError> {
        Err(runner::ExecutionError::new(socket_error))
    };
    let result = runner::run(
        &mut executor,
        runner::RunOptions {
            flow: flow.clone(),
            inputs,
            dry_run: dry,
        },
    );
    match result {
        Ok(report) => {
            let flow_id = format!("flow-{}", random_hex8());
            let mut fields = vec![
                ("flow_id", "flowid", Value::String(flow_id)),
                ("name", "name", Value::String(report.name.clone())),
                ("version", "version", Value::from(report.version)),
                (
                    if report.dry_run { "dry_run" } else { "" },
                    "dryrun",
                    Value::Bool(report.dry_run),
                ),
                (
                    if report.steps.is_empty() { "" } else { "steps" },
                    "steps",
                    Value::Array(Vec::new()),
                ),
            ];
            let plan: Vec<Value> = report
                .plan
                .iter()
                .map(|item| {
                    go_struct(vec![
                        ("index", "index", Value::from(item.index)),
                        ("action", "action", Value::String(item.action.clone())),
                        (
                            "risk_class",
                            "riskclass",
                            Value::String(item.risk_class.clone()),
                        ),
                    ])
                })
                .collect();
            fields.push((
                if plan.is_empty() { "" } else { "plan" },
                "plan",
                Value::Array(plan),
            ));
            fields.push((
                if report.outputs.is_empty() {
                    ""
                } else {
                    "outputs"
                },
                "outputs",
                json!(report.outputs),
            ));
            fields.push(("success", "success", Value::Bool(report.success)));
            fields.push(("", "error", Value::String(String::new())));
            fields.push((
                "duration_ms",
                "durationms",
                Value::from(report.duration_ms as u64),
            ));
            ctx.write_ok(go_struct(fields))
        }
        Err(error) if error.action == "inputs" => Err(CliError::plain(error.message)),
        Err(error) => {
            let step = flow.steps.get(error.step_index);
            let domain_error = error.message.starts_with("domain ")
                || error.message.starts_with("cannot parse open URL");
            let message = error.message.clone();
            let diagnosis_message = if domain_error {
                format!(
                    "flow step {} ({}) failed: {message}",
                    error.step_index, error.action
                )
            } else {
                message.clone()
            };
            let (selector, expected, repair) =
                step.map_or_else(Default::default, |step| diagnose(step, &diagnosis_message));
            let actual = step.map_or(String::new(), observe_actual);
            let hint = if domain_error {
                "add the domain to the flow's domains list or fix the URL".to_owned()
            } else {
                repair.clone()
            };
            let mut diagnosis = Map::new();
            diagnosis.insert("step_index".into(), json!(error.step_index));
            diagnosis.insert("action".into(), json!(error.action));
            if !selector.is_empty() {
                diagnosis.insert("selector".into(), json!(selector));
            }
            if !expected.is_empty() {
                diagnosis.insert("expected".into(), json!(expected));
            }
            if !actual.is_empty() {
                diagnosis.insert("actual".into(), json!(actual));
            }
            diagnosis.insert("message".into(), json!(diagnosis_message));
            if !repair.is_empty() {
                diagnosis.insert("repair_suggestion".into(), json!(repair));
            }
            let details = json!({
                "step_index": error.step_index,
                "action": error.action,
                "message": message,
                "hint": hint,
                "diagnosis": diagnosis_value(&diagnosis),
            });
            let mut envelope =
                symbrowse_core::output::Envelope::failure(ErrorCode::FlowFailed, "flow run failed");
            if let Some(payload) = envelope.error.as_mut() {
                payload.details = Some(details);
            }
            ctx.write_envelope(&envelope)?;
            Err(CliError::plain(format!(
                "flow step {} ({}) failed: {message}",
                error.step_index, error.action
            )))
        }
    }
}

/// Diagnosis fields in `Diagnosis` struct order (JSON omits empty optional
/// fields, YAML keeps every field under its lower-cased Go name).
fn diagnosis_value(map: &Map<String, Value>) -> Value {
    let fields = [
        ("step_index", "stepindex", false),
        ("action", "action", false),
        ("selector", "selector", true),
        ("expected", "expected", true),
        ("actual", "actual", true),
        ("message", "message", false),
        ("snapshot_diff", "snapshotdiff", true),
        ("repair_suggestion", "repairsuggestion", true),
    ];
    go_struct(
        fields
            .iter()
            .map(|(json, yaml, optional)| {
                let value = map.get(*json).cloned();
                let empty = value
                    .as_ref()
                    .is_none_or(|v| v.as_str().is_some_and(str::is_empty));
                let json_name = if *optional && empty { "" } else { *json };
                (
                    json_name,
                    *yaml,
                    value.unwrap_or_else(|| Value::String(String::new())),
                )
            })
            .collect(),
    )
}

fn random_hex8() -> String {
    let mut bytes = [0u8; 8];
    if let Ok(mut file) = fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = file.read_exact(&mut bytes);
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn finder_description(step: &flows::Step) -> String {
    for name in [
        "label",
        "role",
        "text",
        "placeholder",
        "alt",
        "title",
        "testid",
    ] {
        let value = step.field(name);
        if !value.is_empty() {
            return format!("{name}={}", go_quote(value));
        }
    }
    String::new()
}

fn first_non_empty(step: &flows::Step, names: &[&str]) -> String {
    names
        .iter()
        .map(|name| step.field(name))
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_owned()
}

fn wait_description(step: &flows::Step) -> String {
    if !step.field("url").is_empty() {
        format!("url={}", step.field("url"))
    } else if !step.field("visible").is_empty() {
        format!("visible={}", step.field("visible"))
    } else {
        format!("ms={}", step.field("ms").parse::<i64>().unwrap_or(0))
    }
}

fn assert_description(step: &flows::Step) -> String {
    for name in ["visible", "url", "text", "not"] {
        let value = step.field(name);
        if !value.is_empty() {
            return format!("{name}={value}");
        }
    }
    String::new()
}

fn observe_actual(step: &flows::Step) -> String {
    // The flow executor never reaches a daemon, so `get.url` lookups fail and
    // only the unconditional "element not found" observation remains.
    if step.action == "assert" && !step.field("visible").is_empty() && step.field("url").is_empty()
    {
        return "element not found".into();
    }
    String::new()
}

/// `(selector, expected, repair_suggestion)` of `diagnoseFailure`.
fn diagnose(step: &flows::Step, message: &str) -> (String, String, String) {
    let selector = match step.action.as_str() {
        "open" => step.field("url").to_owned(),
        "find" => finder_description(step),
        "click" | "fill" => first_non_empty(step, &["label", "role", "text", "name"]),
        "wait" => wait_description(step),
        "assert" => assert_description(step),
        _ => String::new(),
    };
    let expected = match step.action.as_str() {
        "assert" => assert_description(step),
        "wait" => wait_description(step),
        "find" if !step.field("action").is_empty() => {
            format!("{} {}", step.field("action"), finder_description(step))
        }
        "click" => "element clickable".to_owned(),
        "fill" => "element fillable".to_owned(),
        _ => String::new(),
    };
    let lower = message.to_lowercase();
    let repair = match step.action.as_str() {
        "wait" => "the wait condition was not met; extend the wait or adjust the condition (url/visible/ms)".to_owned(),
        "assert" => "the assertion failed; compare the expected state with the snapshot diff and adjust the selector or the assertion".to_owned(),
        "open" => {
            if lower.contains("not allowed") {
                "the URL domain is outside the flow's domains list; add the domain or fix the URL".to_owned()
            } else {
                "the page could not be opened; check the URL and network reachability".to_owned()
            }
        }
        "find" | "click" | "fill" => {
            if lower.contains("matched no elements") || lower.contains("not found") {
                format!("selector {} matched no element; check the label/role/text against the current page (see snapshot diff)", go_quote(&selector))
            } else if lower.contains("matched") && lower.contains("elements") {
                format!("selector {} is ambiguous; add name=, exact: true, or a more specific role to disambiguate", go_quote(&selector))
            } else if lower.contains("obstructed") || lower.contains("covered") || lower.contains("overlay") {
                "another element covers the target; use a more specific selector or dismiss the overlay first".to_owned()
            } else if lower.contains("stale") {
                "the element ref is stale; the page changed between steps — add a wait or snapshot step before this one".to_owned()
            } else {
                "verify the selector against the snapshot diff; the page may have changed".to_owned()
            }
        }
        _ => String::new(),
    };
    (selector, expected, repair)
}

// ---------------------------------------------------------------------------
// doctor

fn doctor(ctx: &mut Ctx<'_>) -> Outcome {
    done_or_exit(doctor_inner(ctx))
}

fn done_or_exit(result: Result<(), CliError>) -> Outcome {
    match result {
        Ok(()) => Outcome::Done,
        Err(error) => Outcome::Fail(error),
    }
}

struct Check {
    name: &'static str,
    status: &'static str,
    message: String,
    details: Vec<(String, String)>,
}

impl Check {
    fn value(&self) -> Value {
        let mut details = Map::new();
        for (key, value) in &self.details {
            details.insert(key.clone(), Value::String(value.clone()));
        }
        go_struct(vec![
            ("name", "name", json!(self.name)),
            ("status", "status", json!(self.status)),
            ("message", "message", json!(self.message)),
            (
                if details.is_empty() { "" } else { "details" },
                "details",
                Value::Object(details),
            ),
        ])
    }
}

fn check(
    name: &'static str,
    status: &'static str,
    message: impl Into<String>,
    details: &[(&str, &str)],
) -> Check {
    Check {
        name,
        status,
        message: message.into(),
        details: details
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    }
}

fn search_paths(home: &str) -> Vec<String> {
    let (known, names): (Vec<String>, &[&str]) = if cfg!(target_os = "macos") {
        (
            vec![
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into(),
                format!("{home}/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
                "/Applications/Chromium.app/Contents/MacOS/Chromium".into(),
                format!("{home}/Applications/Chromium.app/Contents/MacOS/Chromium"),
                "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge".into(),
                format!("{home}/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
            ],
            &[
                "google-chrome",
                "google-chrome-stable",
                "chromium",
                "chromium-browser",
                "microsoft-edge",
                "microsoft-edge-stable",
            ],
        )
    } else {
        (
            [
                "/usr/bin/google-chrome",
                "/usr/bin/google-chrome-stable",
                "/usr/bin/chromium",
                "/usr/bin/chromium-browser",
                "/usr/bin/microsoft-edge",
                "/usr/bin/microsoft-edge-stable",
                "/opt/google/chrome/google-chrome",
                "/opt/microsoft/msedge/msedge",
                "/snap/bin/chromium",
            ]
            .map(str::to_owned)
            .to_vec(),
            &[
                "google-chrome",
                "google-chrome-stable",
                "chromium",
                "chromium-browser",
                "microsoft-edge",
                "microsoft-edge-stable",
            ],
        )
    };
    let mut paths = known;
    paths.push(format!("PATH: {}", names.join(", ")));
    paths
}

fn usable(path: &str) -> bool {
    fs::metadata(path).is_ok_and(|info| info.is_file() && info.permissions().mode() & 0o111 != 0)
}

fn look_path(name: &str) -> Option<String> {
    if name.contains('/') {
        return usable(name).then(|| name.to_owned());
    }
    std::env::var("PATH").ok()?.split(':').find_map(|dir| {
        let dir = if dir.is_empty() { "." } else { dir };
        let candidate = format!("{dir}/{name}");
        usable(&candidate).then_some(candidate)
    })
}

fn discover(override_path: &str, home: &str) -> Option<(String, &'static str)> {
    let names = [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "microsoft-edge",
        "microsoft-edge-stable",
    ];
    if !override_path.is_empty() {
        let candidate = if override_path.starts_with('/') {
            override_path.to_owned()
        } else {
            look_path(override_path)?
        };
        return usable(&candidate).then_some((candidate, "SYMBROWSE_EXECUTABLE_PATH"));
    }
    let search = search_paths(home);
    for candidate in search.iter().filter(|p| !p.starts_with("PATH: ")) {
        if usable(candidate) {
            return Some((candidate.clone(), "platform path"));
        }
    }
    names
        .iter()
        .find_map(|name| look_path(name).map(|path| (path, "PATH")))
}

fn check_version(path: &str) -> Check {
    let child = std::process::Command::new(path)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output();
    match child {
        Err(error) => check(
            "version",
            "warn",
            format!("version check failed: {}", go_errno(&error)),
            &[("path", path)],
        ),
        Ok(output) if !output.status.success() => check(
            "version",
            "warn",
            format!("version check failed: {}", exit_status_text(&output.status)),
            &[("path", path)],
        ),
        Ok(output) => {
            let mut combined = output.stdout;
            combined.extend_from_slice(&output.stderr);
            let text = String::from_utf8_lossy(&combined);
            let version = text
                .split('\n')
                .next()
                .unwrap_or_default()
                .trim()
                .to_owned();
            if version.is_empty() {
                check(
                    "version",
                    "warn",
                    "browser returned an empty version",
                    &[("path", path)],
                )
            } else {
                check(
                    "version",
                    "pass",
                    version.clone(),
                    &[("path", path), ("version", &version)],
                )
            }
        }
    }
}

fn exit_status_text(status: &std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit status {code}"),
        None => "signal: killed".into(),
    }
}

fn check_writable(name: &'static str, path: &str) -> Check {
    if path.is_empty() {
        return check(name, "fail", "path is empty", &[]);
    }
    let mut probe = PathBuf::from(path);
    let mut missing = false;
    loop {
        match fs::metadata(&probe) {
            Ok(info) => {
                if !info.is_dir() {
                    return check(
                        name,
                        "fail",
                        "path exists but is not a directory",
                        &[("path", path)],
                    );
                }
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing = true;
                match probe.parent() {
                    Some(parent) if parent != probe && !parent.as_os_str().is_empty() => {
                        probe = parent.to_path_buf();
                    }
                    _ => {
                        return check(
                            name,
                            "fail",
                            "no existing parent directory",
                            &[("path", path)],
                        );
                    }
                }
            }
            Err(error) => {
                return check(
                    name,
                    "fail",
                    format!(
                        "cannot inspect path: {}",
                        io_path_error("stat", path, &error)
                    ),
                    &[("path", path)],
                );
            }
        }
    }
    let probe_text = probe.to_string_lossy().into_owned();
    let file = probe.join(format!(".symbrowse-doctor-{}", random_hex8()));
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&file)
    {
        Err(error) => check(
            name,
            "fail",
            format!(
                "directory is not writable: {}",
                io_path_error("open", &file.to_string_lossy(), &error)
            ),
            &[("path", path), ("probe_dir", &probe_text)],
        ),
        Ok(handle) => {
            drop(handle);
            if fs::remove_file(&file).is_err() {
                return check(
                    name,
                    "fail",
                    "temporary write probe could not be cleaned up",
                    &[("path", path), ("probe_dir", &probe_text)],
                );
            }
            if missing {
                check(
                    name,
                    "warn",
                    "path does not exist yet; nearest existing parent is writable",
                    &[("path", path), ("probe_dir", &probe_text)],
                )
            } else {
                check(name, "pass", "directory is writable", &[("path", path)])
            }
        }
    }
}

fn check_key_source() -> Check {
    use symbrowse_core::{key_resolver::KeyResolver, key_sources::SystemKeySources};
    let resolver = KeyResolver::new(SystemKeySources::default());
    match resolver.resolve() {
        Err(error) => check(
            "key_source",
            "warn",
            format!(
                "state encryption key source could not be resolved; run `symbrowse state key init` after fixing the provider: {error}"
            ),
            &[("fix", "symbrowse state key init")],
        ),
        Ok(None) => check(
            "key_source",
            "warn",
            "no state encryption key configured (key_source: none); run `symbrowse state key init` to enable encryption",
            &[("source", "none"), ("fix", "symbrowse state key init")],
        ),
        Ok(Some(material)) => {
            let source = material.source().to_owned();
            check(
                "key_source",
                "pass",
                format!("state encryption key resolved from {source}"),
                &[("source", &source)],
            )
        }
    }
}

fn check_cdp(endpoint: &str) -> Check {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    let Ok(runtime) = runtime else {
        return check(
            "cdp",
            "warn",
            "CDP is not running or unreachable",
            &[("endpoint", endpoint)],
        );
    };
    let timeout = Duration::from_millis(750);
    let outcome = runtime.block_on(async {
        let client = reqwest::Client::builder().timeout(timeout).build()?;
        client.get(endpoint).send().await
    });
    match outcome {
        Err(error) if error.is_builder() => check(
            "cdp",
            "warn",
            format!("CDP endpoint is invalid: {error}"),
            &[("endpoint", endpoint)],
        ),
        Err(error) => check(
            "cdp",
            "warn",
            format!("CDP is not running or unreachable: {error}"),
            &[("endpoint", endpoint)],
        ),
        Ok(response) if !response.status().is_success() => check(
            "cdp",
            "warn",
            format!("CDP endpoint returned HTTP {}", response.status()),
            &[("endpoint", endpoint)],
        ),
        Ok(_) => check(
            "cdp",
            "pass",
            "CDP endpoint is reachable; the daemon would attach to this browser (issue #296)",
            &[("endpoint", endpoint), ("mode", "attach")],
        ),
    }
}

fn doctor_inner(ctx: &mut Ctx<'_>) -> Handled {
    let config = load_config()?;
    let home = std::env::var("HOME").unwrap_or_default();
    let socket_dir = if cfg!(target_os = "macos") {
        format!("{home}/Library/Caches/symbrowse/run")
    } else {
        let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
        let base = if base.is_empty() {
            format!("{home}/.cache/symbrowse/run")
        } else {
            base
        };
        format!("{base}/symbrowse")
    };
    let interfaces = symbrowse_engine::capabilities::OPTIONAL_INTERFACE_NAMES.join(",");
    let mut checks = vec![check(
        "engine",
        "pass",
        format!(
            "engine \"chrome\" (launch): {} optional interface(s) implemented, 0 unsupported",
            symbrowse_engine::capabilities::OPTIONAL_INTERFACE_NAMES.len()
        ),
        &[
            ("interfaces", &interfaces),
            ("kind", "chrome"),
            ("launch_mode", "launch"),
            ("unsupported", ""),
        ],
    )];
    let search = search_paths(&home);
    match discover(&config.executable_path, &home) {
        None => {
            checks.push(check(
                "chrome",
                "fail",
                format!(
                    "Chrome, Chromium, or Edge was not found; searched {}; set SYMBROWSE_EXECUTABLE_PATH to override discovery",
                    search.join(", ")
                ),
                &[("override", "SYMBROWSE_EXECUTABLE_PATH"), ("search_paths", &search.join("\n"))],
            ));
            checks.push(check(
                "version",
                "skipped",
                "skipped because no browser executable was found",
                &[],
            ));
        }
        Some((path, source)) => {
            checks.push(check(
                "chrome",
                "pass",
                format!("using {path} ({source})"),
                &[("path", &path), ("source", source)],
            ));
            checks.push(check_version(&path));
        }
    }
    let endpoint = std::env::var("SYMBROWSE_CDP_ENDPOINT").unwrap_or_default();
    if endpoint.is_empty() {
        checks.push(check(
            "cdp",
            "skipped",
            "not checked: no CDP endpoint configured; daemon sessions launch their own browser (set SYMBROWSE_CDP_ENDPOINT or --cdp-endpoint to attach instead)",
            &[],
        ));
    } else {
        checks.push(check_cdp(&endpoint));
    }
    checks.push(check_writable("config_dir", &config.config_dir));
    checks.push(check_writable("cache_dir", &config.cache_dir));
    checks.push(check_writable("state_dir", &config.state_dir));
    checks.push(check_writable("socket_dir", &socket_dir));
    checks.push(check_key_source());
    checks.push(guard_check());
    let status = if checks.iter().any(|c| c.status == "fail") {
        "fail"
    } else if checks.iter().any(|c| c.status == "warn") {
        "warn"
    } else {
        "pass"
    };
    let fixes = ctx
        .flags
        .bool("fix")
        .then(|| fix_instructions(&config.executable_path));
    if ctx.structured() {
        let mut fields = vec![
            ("status", "status", json!(status)),
            (
                "checks",
                "checks",
                Value::Array(checks.iter().map(Check::value).collect()),
            ),
        ];
        fields.push((
            if fixes.is_some() { "fixes" } else { "" },
            "fixes",
            json!(fixes.clone().unwrap_or_default()),
        ));
        ctx.write_ok(go_struct(fields))?;
    } else {
        for item in &checks {
            let line = format!(
                "[{}] {}: {}",
                item.status.to_uppercase(),
                item.name,
                item.message
            );
            ctx.println(&line);
        }
        if let Some(fixes) = &fixes {
            ctx.println("\nCopyable next steps:");
            for fix in fixes {
                ctx.println(&format!("- {fix}"));
            }
        }
    }
    if let Some(chrome) = checks
        .iter()
        .find(|c| c.name == "chrome" && c.status == "fail")
    {
        return Err(CliError::wrapf(
            5,
            ErrorCode::NotFound,
            chrome.message.clone(),
        ));
    }
    if checks.iter().any(|c| c.status == "fail") {
        return Err(CliError::wrapf(
            1,
            ErrorCode::Internal,
            "one or more doctor checks failed",
        ));
    }
    Ok(())
}

fn guard_check() -> Check {
    use symbrowse_core::policy_guard::{GUARD_BINARY_NAME, GUARD_ENV_NAME, Guard};
    match Guard::detect().filter(Guard::active) {
        Some(guard) => {
            let command = guard.command();
            let path = guard.executable.to_string_lossy().into_owned();
            check(
                "guard",
                "pass",
                format!("risk decisions delegated to {command}"),
                &[
                    ("binary", GUARD_BINARY_NAME),
                    ("command", &command),
                    ("path", &path),
                ],
            )
        }
        None => check(
            "guard",
            "warn",
            format!(
                "no external risk decider found ({GUARD_BINARY_NAME} not on PATH and {GUARD_ENV_NAME} unset); risk decisions fall back to the built-in policy"
            ),
            &[("override", GUARD_ENV_NAME)],
        ),
    }
}

fn fix_instructions(executable: &str) -> Vec<String> {
    let override_path = if executable.is_empty() {
        "/absolute/path/to/Chrome"
    } else {
        executable
    };
    let install = if cfg!(target_os = "macos") {
        "Install Chrome with: brew install --cask google-chrome (or install Chromium/Edge from its official macOS installer)."
    } else if cfg!(target_os = "linux") {
        "Install Chromium with: sudo apt install chromium (or use your distribution's Chromium/Chrome/Edge package)."
    } else {
        "Install Google Chrome, Chromium, or Microsoft Edge using your platform's package manager or official installer."
    };
    vec![
        "No changes were made; --fix only prints guidance.".to_owned(),
        install.to_owned(),
        format!(
            "Set an explicit browser when it is installed elsewhere: export SYMBROWSE_EXECUTABLE_PATH='{}'",
            override_path.replace('\'', "'\\\"'\\\"'")
        ),
        "Rerun the checks with: symbrowse doctor (or symbrowse doctor --json).".to_owned(),
    ]
}

// ---------------------------------------------------------------------------
// flow list / flow validate

fn flow_list(ctx: &mut Ctx<'_>) -> Handled {
    let mut groups = Vec::new();
    let project = Path::new(".symbrowse").join("flows");
    if let Ok(found) = flows::discover_directory(&project, "project") {
        groups.push(found);
    }
    if let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
        let global = Path::new(&home).join(".config/symbrowse/flows");
        if let Ok(found) = flows::discover_directory(&global, "global") {
            groups.push(found);
        }
    }
    let list: Vec<Value> = flows::merge_discovered(groups)
        .into_iter()
        .map(|flow| {
            go_struct(vec![
                ("name", "name", json!(flow.name)),
                ("path", "path", json!(flow.path)),
                ("origin", "origin", json!(flow.origin)),
                ("version", "version", json!(flow.version)),
                ("steps", "steps", json!(flow.steps)),
            ])
        })
        .collect();
    ctx.write_ok(json!({"flows": list}))
}

fn flow_validate(ctx: &mut Ctx<'_>, args: &[String]) -> Handled {
    let path = &args[0];
    let data = fs::read(path).map_err(|error| {
        CliError::plain(format!(
            "read flow {}: {}",
            go_quote(path),
            io_path_error("open", path, &error)
        ))
    })?;
    match flows::parse(&data, path.clone()) {
        Err(error) => {
            let details: Vec<Value> = error
                .errors
                .iter()
                .map(|e| {
                    go_struct(vec![
                        ("line", "line", json!(e.line)),
                        ("field", "field", json!(e.field)),
                        ("reason", "reason", json!(e.reason)),
                    ])
                })
                .collect();
            let mut envelope = symbrowse_core::output::Envelope::failure(
                ErrorCode::Validation,
                "flow validation failed",
            );
            if let Some(payload) = envelope.error.as_mut() {
                payload.details = Some(json!({"errors": details}));
            }
            ctx.write_envelope(&envelope)?;
            Err(CliError::wrapf(
                2,
                ErrorCode::Validation,
                format!("flow validation failed ({} error(s))", error.errors.len()),
            ))
        }
        Ok(flow) => {
            if ctx.structured() {
                return ctx.write_ok(json!({
                    "valid": true,
                    "name": flow.name,
                    "version": flow.version,
                    "steps": flow.steps.len(),
                    "domains": flow.domains,
                    "source": path,
                    "schema": "docs/flow-schema.json",
                    "schema_id": "https://symaira.dev/schemas/symbrowse-flow.json",
                }));
            }
            let line = format!(
                "valid: {} (version {}, {} steps, domains [{}])",
                flow.name,
                flow.version,
                flow.steps.len(),
                flow.domains.join(" ")
            );
            ctx.println(&line);
            Ok(())
        }
    }
}
