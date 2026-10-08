//! Guard doctor discovery, spawn allowlist and plaintext-secret findings.
use super::config::SpawnEntry;
use crate::guard_scan::{self, guard_scan_config};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug)]
pub(super) struct Discovered {
    pub(super) name: String,
    pub(super) client: String,
    pub(super) command: String,
    pub(super) args: Vec<String>,
    pub(super) transport: String,
    pub(super) env: BTreeMap<String, String>,
}

/// Ports `discovery.DiscoverAll()`. Missing files are skipped except for
/// Windows path-not-found errors, which Go's string-based missing-file check
/// surfaces as an `mcp servers error: discovery: …` report. Unreproducible
/// parser errors still gate to the Go fallback.
pub(super) enum DiscoveryOutcome {
    Servers(Vec<Discovered>),
    Error(String),
}

pub(super) fn discover_all() -> Option<DiscoveryOutcome> {
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
pub(super) fn looks_like_secret(key: &str, value: &str) -> bool {
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
pub(super) fn is_env_reference(value: &str) -> bool {
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
pub(super) fn allows(server: &Discovered, allowlist: &[SpawnEntry]) -> bool {
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
pub(super) fn clean_path(path: &str) -> String {
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

pub(super) struct ServerCheck {
    pub(super) name: String,
    pub(super) client: String,
    pub(super) command: String,
    pub(super) args: Vec<String>,
    pub(super) transport: String,
    pub(super) allowed: bool,
    pub(super) secrets: Vec<String>,
}

/// Ports `checkServers`. Secret keys come out sorted because `env` is a
/// `BTreeMap`; Go's order is a map range (#640).
pub(super) fn check_servers(
    servers: Vec<Discovered>,
    allowlist: &[SpawnEntry],
) -> Vec<ServerCheck> {
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
    checks.sort_by(|a, b| a.client.cmp(&b.client).then_with(|| a.name.cmp(&b.name)));
    checks
}

/// Ports `printServerChecks`.
pub(super) fn print_server_checks(out: &mut String, checks: &[ServerCheck]) {
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
pub(super) fn print_secret_risks(out: &mut String, checks: &[ServerCheck]) {
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
