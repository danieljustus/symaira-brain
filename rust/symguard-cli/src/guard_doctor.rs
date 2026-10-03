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
//!   message.
//!
//! Deliberate deviation (#640): Go prints a server's plaintext secret keys
//! in Go map-range order, which is not stable between two Go runs. Rust
//! prints them sorted by key and handles multi-secret servers natively.

use std::fmt::Write as _;
use std::io::Write;
use symbrain_core::{exit, version};

#[path = "doctor/audit.rs"]
mod audit;
#[path = "doctor/config.rs"]
mod config;
#[path = "doctor/discovery.rs"]
mod discovery;
use audit::audit_status;
use config::{ConfigState, LoadedConfig, config_path, load_config};
use discovery::{
    DiscoveryOutcome, check_servers, discover_all, print_secret_risks, print_server_checks,
};

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
        checks = check_servers(servers, &allowlist);
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

#[cfg(test)]
#[path = "doctor/tests.rs"]
mod tests;
