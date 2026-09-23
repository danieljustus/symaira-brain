//! Native implementation of `symbrain guard doctor`.
//!
//! Ported from `guard/cmd/symguard/doctor/{command,checks}.go` and the Go
//! packages it depends on:
//!
//! * `guard/internal/config` — `ConfigPath`, `DataDir`, the TOML schema,
//!   `Load`'s default-on-missing behavior and `validate`.
//! * `guard/internal/spawn` — `NewAllowlist`/`Allows`/`Len`.
//! * `guard/internal/discovery` — `DiscoverAll`, `Server`,
//!   `PlaintextSecretKeys`, `LooksLikeSecret`.
//! * `guard/internal/audit` — `DefaultAnchorPath` + `ReadCheckpoint`.
//!
//! # Gating
//!
//! Every state whose bytes this port cannot reproduce exactly is detected
//! *before any output is written*, and `run` returns `None` so the caller
//! falls back to the Go binary. A false "needs Go" is always preferred over
//! a false clean report, because a wrong answer here hides real
//! spawn-allowlist and plaintext-secret findings. The gates are:
//!
//! * a config parse/validation error other than TOML's simple missing-`=`
//!   diagnostic — the Rust parser can reproduce only that narrow Go message;
//! * an audit anchor with valid JSON but the wrong `ChainAnchor` shape still
//!   gates, because serde's type errors differ from `encoding/json`;
//! * any discovery source that exists but fails to read or parse, or an
//!   entry with neither `command` nor `url` — Go turns those into an
//!   `mcp servers  error: discovery: …` line carrying the upstream parser's
//!   message;
//! * a single server carrying more than one plaintext secret key — Go emits
//!   those in `EnvKeys` order, which comes from a Go map range and is
//!   therefore not deterministic even between two Go runs (see the report
//!   in the migration notes); there is no byte answer to match.

use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use toml_edit::{DocumentMut, Item, Table, Value};

use symbrain_core::{exit, version};

// The MCP client config parser used by `guard scan`, reused rather than
// duplicated. Go's `guard/internal/discovery` is itself a thin adapter over
// `corekit/mcpcfgkit` — the very implementation `guard_scan_config` already
// ports, path list and all (verified directly against the pinned
// `symaira-corekit v0.17.0` module source) — so a second copy would only be
// a second place to drift on a security-sensitive list.
use super::guard_scan::{self, guard_scan_config};

/// Runs `symbrain guard doctor`. Returns `None` when any part of this
/// machine's state cannot be reproduced byte-for-byte, so the caller falls
/// back to the Go binary.
pub(crate) fn run(stdout: &mut dyn Write) -> Option<u8> {
    let (report, code) = build_report()?;
    let _ = stdout.write_all(report.as_bytes());
    Some(code)
}

/// Builds the whole report in memory. `None` means "fall back to Go" — no
/// byte is written in that case, by construction.
fn build_report() -> Option<(String, u8)> {
    let config = load_config(&config_path())?;
    let audit = audit_status(&super::audit_path())?;
    let (config_status, loaded) = match config {
        ConfigState::Missing => (
            "not configured (no config file found)".to_owned(),
            Some(LoadedConfig::default()),
        ),
        ConfigState::Loaded(loaded) => ("ok".to_owned(), Some(loaded)),
        ConfigState::ParseError(error) => (
            format!("error: config: parse {}: {error}", config_path().display()),
            None,
        ),
    };

    let mut out = String::new();
    let build_version = option_env!("SYMBRAIN_VERSION").unwrap_or("dev");
    out.push_str("symguard doctor\n\n");
    let _ = writeln!(out, "  Version:   {build_version}");
    // Go's own line here is `runtime.Version()`. A Rust binary has no Go
    // toolchain and must never print a fabricated one (a prior wave did
    // exactly that and was reverted): report the real Rust toolchain under
    // its own label, as `guard version` already does.
    let _ = writeln!(out, "  Rust:      {}", crate::rustc_version());
    let _ = writeln!(
        out,
        "  OS/Arch:   {}/{}\n",
        version::current_os(),
        version::current_arch()
    );

    let mut row = |name: &str, status: &str| {
        let _ = writeln!(out, "  {name:<16} {status}");
    };
    row("binary", "ok");
    row("go runtime", "ok");
    row("config", &config_status);
    if let Some(loaded) = &loaded {
        if loaded.rules > 0 {
            row("policy", &format!("ok ({} rule(s))", loaded.rules));
        } else {
            row("policy", "defaults only (no rules — deny by default)");
        }
    } else {
        row("policy", "not loaded (config error)");
    }
    row("audit log", audit.0.as_str());

    if loaded.is_none() {
        let issues = 2 + usize::from(audit.1);
        let _ = writeln!(out, "\n{issues} issue(s) found. See details above.");
        return Some((out, exit::GENERIC));
    }
    let loaded = loaded?;

    let discovery = discover_all()?;
    let (servers, discovery_error) = match discovery {
        DiscoveryOutcome::Servers(servers) => (servers, None),
        DiscoveryOutcome::Error(error) => (Vec::new(), Some(error)),
    };

    let allowlist = loaded.allowlist;
    if allowlist.is_empty() {
        row(
            "spawn allowlist",
            "not configured (empty — deny by default)",
        );
    } else {
        row(
            "spawn allowlist",
            &format!("ok ({} entries)", allowlist.len()),
        );
    }

    let mut checks = Vec::new();
    if let Some(error) = &discovery_error {
        row("mcp servers", &format!("error: {error}"));
    } else if servers.is_empty() {
        row("mcp servers", "none discovered");
    } else {
        row("mcp servers", &format!("{} discovered", servers.len()));
        checks = check_servers(servers, &allowlist)?;
    }

    let discovery_problems = usize::from(discovery_error.is_some());
    let problems = discovery_problems
        + usize::from(audit.1)
        + checks
            .iter()
            .filter(|c| !c.allowed || !c.secrets.is_empty())
            .count();

    print_server_checks(&mut out, &checks);
    print_secret_risks(&mut out, &checks);

    out.push('\n');
    if problems == 0 {
        out.push_str(
            "All basic checks passed. Run 'symguard scan' after setup for full diagnostics.\n",
        );
        return Some((out, exit::OK));
    }
    let _ = writeln!(out, "{problems} issue(s) found. See details above.");
    Some((out, exit::GENERIC))
}

// ---------------------------------------------------------------------------
// config (guard/internal/config)
// ---------------------------------------------------------------------------

/// Mirrors `config.ConfigPath()`/`DefaultPath()`: `$SYMGUARD_CONFIG`, else
/// `$XDG_CONFIG_HOME/symguard/config.toml`, else
/// `~/.config/symguard/config.toml`.
fn config_path() -> PathBuf {
    if let Some(env) = env::var_os("SYMGUARD_CONFIG").filter(|v| !v.is_empty()) {
        return PathBuf::from(env);
    }
    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(xdg).join("symguard").join("config.toml");
    }
    let home = symbrain_core::xdg::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("symguard").join("config.toml")
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SpawnEntry {
    path: String,
    argv_prefix: Vec<String>,
}

#[derive(Debug, Default)]
struct LoadedConfig {
    rules: usize,
    allowlist: Vec<SpawnEntry>,
}

/// The outcomes of `config.Load()` that doctor can reproduce natively.
enum ConfigState {
    Missing,
    Loaded(LoadedConfig),
    ParseError(String),
}

/// Ports `config.Load()`. `None` means Go would print an error string this
/// port cannot reproduce — fall back.
fn load_config(path: &Path) -> Option<ConfigState> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Some(ConfigState::Missing),
        Err(_) => return None,
    };
    let doc = match text.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(error) => {
            let diagnostic = go_missing_equals_diagnostic(&text, &error)?;
            return Some(ConfigState::ParseError(format!("toml: {diagnostic}")));
        }
    };
    parse_and_validate(&doc).map(ConfigState::Loaded)
}

/// Maps only toml_edit's missing-`=` error with a printable ASCII offender;
/// all other parser messages stay on the Go fallback path.
fn go_missing_equals_diagnostic(text: &str, error: &toml_edit::TomlError) -> Option<String> {
    if error.message() != "key with no value, expected `=`" {
        return None;
    }
    let span = error.span()?;
    if !span.is_empty() || !text.is_char_boundary(span.start) {
        return None;
    }
    let byte = *text.as_bytes().get(span.start)?;
    if !(0x20..=0x7e).contains(&byte) || matches!(byte, b'\'' | b'"' | b'\\') {
        return None;
    }
    let line = text.as_bytes()[..span.start]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1;
    Some(format!(
        "line {line}: expected '.' or '=', but got '{}' instead",
        char::from(byte)
    ))
}

const VALID_DECISIONS: [&str; 6] = ["allow", "ask", "deny", "redact", "readonly", "sandbox"];

/// Decodes and validates the TOML exactly as far as doctor's output depends
/// on it. Every shape mismatch (a Go decode error) and every `validate()`
/// rejection returns `None`, because both print library or format text this
/// port does not reproduce.
fn parse_and_validate(doc: &DocumentMut) -> Option<LoadedConfig> {
    check_defaults(doc)?;
    let rules = count_rules(doc)?;
    check_sequence(doc)?;
    check_unprinted_sections(doc)?;
    let allowlist = read_allowlist(doc)?;
    Some(LoadedConfig { rules, allowlist })
}

fn string_of(item: &Item) -> Option<&str> {
    item.as_str()
}

fn string_array(item: &Item) -> Option<Vec<String>> {
    item.as_array()?
        .iter()
        .map(|value| match value {
            Value::String(text) => Some(text.value().clone()),
            _ => None,
        })
        .collect()
}

/// `[defaults]` decodes to `map[string]Decision`; `validate()` rejects any
/// value that is not a known decision.
fn check_defaults(doc: &DocumentMut) -> Option<()> {
    let Some(item) = doc.get("defaults") else {
        return Some(());
    };
    let table = item.as_table()?;
    for (_, value) in table {
        if !VALID_DECISIONS.contains(&string_of(value)?) {
            return None;
        }
    }
    Some(())
}

/// `[[rules]]`: every rule needs a valid decision and at least one match
/// criterion, else `validate()` rejects the file.
fn count_rules(doc: &DocumentMut) -> Option<usize> {
    let Some(item) = doc.get("rules") else {
        return Some(0);
    };
    let tables = item.as_array_of_tables()?;
    for rule in tables {
        if !VALID_DECISIONS.contains(&string_of(rule.get("decision")?)?) {
            return None;
        }
        let matcher = rule.get("match")?.as_table()?;
        let mut criteria = 0usize;
        for field in ["server", "tool", "capability"] {
            if let Some(value) = matcher.get(field)
                && !string_of(value)?.is_empty()
            {
                criteria += 1;
            }
        }
        if let Some(value) = matcher.get("command_contains")
            && !string_array(value)?.is_empty()
        {
            criteria += 1;
        }
        if criteria == 0 {
            return None;
        }
    }
    Some(tables.len())
}

/// `[sequence]`: `DefaultConfig()` seeds `Threshold = 3`, and a TOML decode
/// only overwrites keys the file actually contains.
fn check_sequence(doc: &DocumentMut) -> Option<()> {
    let Some(item) = doc.get("sequence") else {
        return Some(());
    };
    let table = item.as_table()?;
    let enabled = match table.get("enabled") {
        None => false,
        Some(value) => value.as_bool()?,
    };
    let threshold = match table.get("threshold") {
        None => 3,
        Some(value) => value.as_integer()?,
    };
    if enabled && threshold < 2 {
        return None;
    }
    Some(())
}

/// `[proxy]`, `[audit]` and `[[remote]]` never reach doctor's output, but a
/// wrong type in any of them is a Go decode error, so their shape still
/// decides native versus Go.
fn check_unprinted_sections(doc: &DocumentMut) -> Option<()> {
    fn strings(table: &Table, fields: &[&str]) -> Option<()> {
        for field in fields {
            if let Some(value) = table.get(field) {
                string_of(value)?;
            }
        }
        Some(())
    }
    if let Some(item) = doc.get("proxy") {
        strings(item.as_table()?, &["upstream"])?;
    }
    if let Some(item) = doc.get("audit") {
        let table = item.as_table()?;
        strings(table, &["path", "encrypt_age"])?;
        if let Some(value) = table.get("encrypt") {
            value.as_bool()?;
        }
    }
    if let Some(item) = doc.get("remote") {
        for target in item.as_array_of_tables()? {
            strings(target, &["name", "provider", "host", "trust_level"])?;
            for field in ["allowed_servers", "labels"] {
                if let Some(value) = target.get(field) {
                    string_array(value)?;
                }
            }
        }
    }
    Some(())
}

/// `[[spawn.allowlist]]`: `validate()` requires a non-empty absolute path.
fn read_allowlist(doc: &DocumentMut) -> Option<Vec<SpawnEntry>> {
    let mut allowlist = Vec::new();
    let Some(item) = doc.get("spawn") else {
        return Some(allowlist);
    };
    let Some(entries) = item.as_table()?.get("allowlist") else {
        return Some(allowlist);
    };
    for entry in entries.as_array_of_tables()? {
        let path = string_of(entry.get("path")?)?.to_owned();
        if path.is_empty() || !Path::new(&path).is_absolute() {
            return None;
        }
        let argv_prefix = match entry.get("argv_prefix") {
            None => Vec::new(),
            Some(value) => string_array(value)?,
        };
        allowlist.push(SpawnEntry { path, argv_prefix });
    }
    Some(allowlist)
}

// ---------------------------------------------------------------------------
// audit log (guard/internal/audit)
// ---------------------------------------------------------------------------

/// Ports doctor's audit-log branch. `None` gates to Go.
fn audit_status(log_path: &Path) -> Option<(String, bool)> {
    match fs::metadata(log_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Some((
                "not initialized (created on first 'symguard decide')".to_owned(),
                false,
            ));
        }
        // Go prints `error: <the os.Stat error>` here; not reproducible.
        Err(_) => return None,
        Ok(_) => {}
    }
    // audit.DefaultAnchorPath(logPath) == logPath + ".anchor"
    let mut anchor_path = log_path.as_os_str().to_owned();
    anchor_path.push(".anchor");
    let anchor_path = PathBuf::from(anchor_path);
    let anchor = match fs::read(&anchor_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(_) => return None,
        Ok(data) => Some(data),
    };
    match anchor {
        None => Some((
            "ok (JSONL, chain anchor pending Phase 3 sink)".to_owned(),
            false,
        )),
        Some(data) => {
            // Syntax errors come from the shared Go-compatible scanner. Keep
            // valid-JSON type/shape failures gated because serde diagnostics
            // do not match encoding/json.
            if let Err(error) =
                symbrain_guard_core::external_decision::validate_go_json_syntax(&data)
            {
                return Some((
                    format!(
                        "error: anchor {}: auditkit: parse anchor: {error}",
                        anchor_path.display()
                    ),
                    true,
                ));
            }
            serde_json::from_slice::<ChainAnchor>(&data).ok()?;
            Some(("ok (hash-chained, anchor present)".to_owned(), false))
        }
    }
}

/// Mirrors `auditkit.ChainAnchor`'s JSON shape closely enough that a
/// document Go would reject is rejected here too (and therefore gated).
#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct ChainAnchor {
    #[serde(default)]
    last_entry_hash: String,
    #[serde(default)]
    entry_count: i64,
    #[serde(default)]
    schema_version: i32,
    #[serde(default)]
    log_size: i64,
    #[serde(default)]
    content_hash: String,
}

// ---------------------------------------------------------------------------
// discovery (guard/internal/discovery)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Discovered {
    name: String,
    client: String,
    command: String,
    args: Vec<String>,
    transport: String,
    env: BTreeMap<String, String>,
}

/// Ports `discovery.DiscoverAll()`. Missing files are skipped except for
/// Windows path-not-found errors, which Go's string-based missing-file check
/// surfaces as an `mcp servers error: discovery: …` report. Unreproducible
/// parser errors still gate to the Go fallback.
enum DiscoveryOutcome {
    Servers(Vec<Discovered>),
    Error(String),
}

fn discover_all() -> Option<DiscoveryOutcome> {
    let mut servers = Vec::new();
    for source in guard_scan::SOURCES {
        let path = guard_scan::source_path(source);
        let data = match fs::read(&path) {
            Ok(data) => data,
            Err(error) if guard_scan::missing_source_is_silent(&error) => continue,
            Err(error) => {
                return Some(DiscoveryOutcome::Error(format!(
                    "discovery: {} ({}): [unsupported] {}",
                    source.client,
                    path.display(),
                    guard_scan::read_error_message(&path, &error)
                )));
            }
        };
        let entries = guard_scan_config::parse_config(&data, source.key).ok()?;
        for (name, entry) in entries {
            let command = entry.command_or_url();
            if command.is_empty() {
                // Go: StatusUnsupported "server %q is missing both command
                // and url" → DiscoverAll returns an error.
                return None;
            }
            servers.push(Discovered {
                name,
                client: source.client.to_owned(),
                command,
                args: entry.args.clone(),
                transport: entry.transport(),
                env: entry.merged_env(),
            });
        }
    }
    Some(DiscoveryOutcome::Servers(servers))
}

// ---------------------------------------------------------------------------
// secrets (guard/internal/discovery/secrets.go)
// ---------------------------------------------------------------------------

const SECRET_KEY_MARKERS: [&str; 8] = [
    "API_KEY",
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "CREDENTIAL",
    "PRIVATE_KEY",
    "AUTH",
];

const SECRET_VALUE_PREFIXES: [&str; 7] = ["sk-", "sk_", "ghp_", "gho_", "AKIA", "xoxb-", "xoxp-"];

/// Ports `discovery.LooksLikeSecret`.
fn looks_like_secret(key: &str, value: &str) -> bool {
    if value.is_empty() || is_env_reference(value) {
        return false;
    }
    let upper = key.to_uppercase();
    if SECRET_KEY_MARKERS
        .iter()
        .any(|marker| upper.contains(marker))
    {
        return true;
    }
    SECRET_VALUE_PREFIXES
        .iter()
        .any(|prefix| value.starts_with(prefix))
}

/// Ports `discovery.isEnvReference`: `$NAME` or `${NAME}`.
fn is_env_reference(value: &str) -> bool {
    let Some(rest) = value.strip_prefix('$') else {
        return false;
    };
    if rest.starts_with('{') && rest.ends_with('}') {
        return true;
    }
    if rest.is_empty() {
        return false;
    }
    rest.chars().all(|c| c == '_' || c.is_ascii_alphanumeric())
}

// ---------------------------------------------------------------------------
// spawn allowlist (guard/internal/spawn)
// ---------------------------------------------------------------------------

/// Ports `spawn.Allowlist.Allows`. Non-stdio servers are never spawned and
/// are always allowed; a stdio server needs an absolute command matching an
/// entry's cleaned path, with the entry's argv prefix matching.
fn allows(server: &Discovered, allowlist: &[SpawnEntry]) -> bool {
    if server.transport != "stdio" {
        return true;
    }
    if !Path::new(&server.command).is_absolute() {
        return false;
    }
    let command = clean_path(&server.command);
    allowlist.iter().any(|entry| {
        Path::new(&entry.path).is_absolute()
            && clean_path(&entry.path) == command
            && entry.argv_prefix.len() <= server.args.len()
            && entry.argv_prefix[..] == server.args[..entry.argv_prefix.len()]
    })
}

/// Ports Go's platform-native `filepath.Clean` for absolute allowlist paths.
fn clean_path(path: &str) -> String {
    let mut cleaned = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                cleaned.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if cleaned.file_name().is_some_and(|name| name != "..") {
                    cleaned.pop();
                } else if !cleaned.has_root() {
                    cleaned.push(component.as_os_str());
                }
            }
        }
    }
    cleaned.to_string_lossy().into_owned()
}

// ---------------------------------------------------------------------------
// checks.go
// ---------------------------------------------------------------------------

struct ServerCheck {
    name: String,
    client: String,
    command: String,
    args: Vec<String>,
    transport: String,
    allowed: bool,
    secrets: Vec<String>,
}

/// Ports `checkServers`. `None` gates: Go emits a server's secret keys in
/// `EnvKeys` order, which is a Go map range and so is not stable even
/// between two Go runs, leaving no byte answer to match for more than one.
fn check_servers(servers: Vec<Discovered>, allowlist: &[SpawnEntry]) -> Option<Vec<ServerCheck>> {
    let mut checks: Vec<ServerCheck> = servers
        .into_iter()
        .map(|server| {
            let allowed = allows(&server, allowlist);
            let secrets = server
                .env
                .iter()
                .filter(|(key, value)| looks_like_secret(key, value))
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            ServerCheck {
                name: server.name,
                client: server.client,
                command: server.command,
                args: server.args,
                transport: server.transport,
                allowed,
                secrets,
            }
        })
        .collect();
    if checks.iter().any(|check| check.secrets.len() > 1) {
        return None;
    }
    checks.sort_by(|a, b| a.client.cmp(&b.client).then_with(|| a.name.cmp(&b.name)));
    Some(checks)
}

/// Ports `printServerChecks`.
fn print_server_checks(out: &mut String, checks: &[ServerCheck]) {
    if checks.is_empty() {
        return;
    }
    out.push_str("\nDiscovered MCP servers (spawn allowlist):\n");
    for check in checks {
        let verdict = if check.transport == "http" {
            "[n/a]    "
        } else if check.allowed {
            "[allowed]"
        } else {
            "[DENIED] "
        };
        let mut desc = format!(
            "{} ({}/{}) → {}",
            check.name, check.client, check.transport, check.command
        );
        if !check.args.is_empty() {
            desc.push(' ');
            desc.push_str(&check.args.join(" "));
        }
        if !check.allowed && check.transport == "stdio" {
            desc.push_str(" (not on spawn allowlist)");
        }
        let _ = writeln!(out, "  {verdict} {desc}");
    }
}

/// Ports `printSecretRisks`. Note it does not depend on `Allowed`: a denied
/// server carrying a plaintext secret prints both blocks.
fn print_secret_risks(out: &mut String, checks: &[ServerCheck]) {
    let mut any = false;
    for check in checks {
        if check.secrets.is_empty() {
            continue;
        }
        if !any {
            any = true;
            out.push_str("\nPlaintext secret risk:\n");
        }
        let _ = writeln!(
            out,
            "  {} ({}): env {} stored as plaintext values in the client config",
            check.name,
            check.client,
            check.secrets.join(", ")
        );
    }
    if any {
        out.push_str("  symguard reports this risk but is not a secret store — move these values to symvault and reference them at launch time.\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_validate_text(text: &str) -> Option<LoadedConfig> {
        let doc = text.parse::<DocumentMut>().ok()?;
        parse_and_validate(&doc)
    }

    fn stdio(command: &str, args: &[&str]) -> Discovered {
        Discovered {
            name: "s".to_owned(),
            client: "cursor".to_owned(),
            command: command.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            transport: "stdio".to_owned(),
            env: BTreeMap::new(),
        }
    }

    fn native_command() -> String {
        #[cfg(windows)]
        {
            let system_root = env::var_os("SystemRoot")
                .or_else(|| env::var_os("windir"))
                .unwrap_or_else(|| r"C:\Windows".into());
            PathBuf::from(system_root)
                .join("System32")
                .join("where.exe")
                .to_string_lossy()
                .into_owned()
        }
        #[cfg(not(windows))]
        {
            "/usr/bin/true".to_owned()
        }
    }

    fn parent_path_variant(path: &str) -> String {
        let path = Path::new(path);
        let parent = path.parent().expect("absolute path has parent");
        let grandparent = parent.parent().expect("test path has grandparent");
        let parent_name = parent.file_name().expect("parent name");
        let file_name = path.file_name().expect("file name");
        grandparent
            .join(parent_name)
            .join("..")
            .join(parent_name)
            .join(file_name)
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn allowlist_is_deny_by_default_and_prefix_matched() {
        let command = native_command();
        let entry = SpawnEntry {
            path: command.clone(),
            argv_prefix: vec!["--once".to_owned()],
        };
        assert!(!allows(&stdio(&command, &["--once"]), &[]));
        assert!(allows(
            &stdio(&command, &["--once", "--extra"]),
            std::slice::from_ref(&entry)
        ));
        assert!(!allows(
            &stdio(&command, &["--twice"]),
            std::slice::from_ref(&entry)
        ));
        // Relative commands can never match an absolute entry.
        assert!(!allows(&stdio("true", &[]), std::slice::from_ref(&entry)));
        // ..-containing paths clean to the same file, as filepath.Clean does.
        assert!(allows(
            &stdio(&parent_path_variant(&command), &["--once"]),
            std::slice::from_ref(&entry)
        ));
        // HTTP servers are not gated at all.
        let mut http = stdio("https://example.test/mcp", &[]);
        http.transport = "http".to_owned();
        assert!(allows(&http, &[]));
    }

    #[test]
    fn secret_heuristic_matches_go() {
        assert!(looks_like_secret("SECRET_KEY", "literal"));
        assert!(looks_like_secret("ANYTHING", "sk-abc"));
        assert!(!looks_like_secret("SECRET_KEY", "${FROM_ENV}"));
        assert!(!looks_like_secret("SECRET_KEY", "$FROM_ENV"));
        assert!(looks_like_secret("SECRET_KEY", "$not-a-reference"));
        assert!(!looks_like_secret("SECRET_KEY", ""));
        assert!(!looks_like_secret("PLAIN", "value"));
    }

    #[test]
    fn config_parses_the_healthy_fixture_and_gates_on_invalid_toml() {
        let command = native_command();
        let command = Value::from(command.as_str());
        let healthy = format!(
            "[defaults]\nshell = \"allow\"\nread_secret = \"deny\"\n\n[[rules]]\nmatch.server = \"symmemory\"\nmatch.tool = \"memory_search\"\ndecision = \"allow\"\n\n[spawn]\n[[spawn.allowlist]]\npath = {command}\n"
        );
        let loaded = parse_and_validate_text(&healthy).expect("healthy config is native");
        assert_eq!(loaded.rules, 1);
        assert_eq!(
            loaded.allowlist,
            vec![SpawnEntry {
                path: native_command(),
                argv_prefix: Vec::new(),
            }]
        );

        // Parsing and validation both fail closed here.
        assert!(parse_and_validate_text("not [valid = toml").is_none());
        // validate() rejections gate too.
        assert!(parse_and_validate_text("[defaults]\nshell = \"nonsense\"\n").is_none());
        assert!(
            parse_and_validate_text("[spawn]\n[[spawn.allowlist]]\npath = \"relative\"\n")
                .is_none()
        );
        assert!(
            parse_and_validate_text("[[rules]]\ndecision = \"allow\"\n[rules.match]\n").is_none()
        );
        assert!(parse_and_validate_text("[sequence]\nenabled = true\nthreshold = 1\n").is_none());
        assert!(parse_and_validate_text("[sequence]\nenabled = true\n").is_some());
    }

    #[test]
    fn unsupported_missing_equals_offenders_stay_gated() {
        for text in [
            "name \"value\"",
            "name 'value'",
            "name \\ value",
            "name \x01 value",
            "name é value",
            "name",
        ] {
            let error = text
                .parse::<DocumentMut>()
                .expect_err("the fixture must be invalid TOML");
            assert_eq!(
                go_missing_equals_diagnostic(text, &error),
                None,
                "must leave {text:?} to Go"
            );
        }
    }

    #[test]
    fn audit_status_reports_all_three_states() {
        let dir = tempfile::tempdir().expect("tempdir");
        let log = dir.path().join("audit.log");
        assert_eq!(
            audit_status(&log).as_ref().map(|status| status.0.as_str()),
            Some("not initialized (created on first 'symguard decide')")
        );
        fs::write(&log, b"{\"entry_id\":\"1\"}\n").expect("write log");
        assert_eq!(
            audit_status(&log).as_ref().map(|status| status.0.as_str()),
            Some("ok (JSONL, chain anchor pending Phase 3 sink)")
        );
        fs::write(
            dir.path().join("audit.log.anchor"),
            br#"{"last_entry_hash":"abc","entry_count":1,"schema_version":2}"#,
        )
        .expect("write anchor");
        assert_eq!(
            audit_status(&log).as_ref().map(|status| status.0.as_str()),
            Some("ok (hash-chained, anchor present)")
        );
        // Syntax errors use the shared Go-compatible JSON scanner.
        fs::write(dir.path().join("audit.log.anchor"), b"not json").expect("write anchor");
        let status = audit_status(&log).expect("syntax error is native");
        assert!(status.0.ends_with(
            "auditkit: parse anchor: invalid character 'o' in literal null (expecting 'u')"
        ));
        assert!(status.1);
        // Valid JSON with an incompatible type remains gated.
        fs::write(
            dir.path().join("audit.log.anchor"),
            br#"{"entry_count":"one"}"#,
        )
        .expect("write anchor");
        assert!(audit_status(&log).is_none());
    }

    #[test]
    fn multiple_secret_keys_on_one_server_gate_to_go() {
        let mut server = stdio("/usr/bin/env", &[]);
        server.env.insert("SECRET_KEY".to_owned(), "a".to_owned());
        server.env.insert("API_KEY".to_owned(), "b".to_owned());
        assert!(check_servers(vec![server], &[]).is_none());
    }
}
