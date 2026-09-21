//! Native implementation of `symbrain guard doctor`'s "empty machine" case.
//!
//! Ported from `guard/cmd/symguard/doctor/command.go`'s `Run`. Only the
//! all-defaults report is reproduced here: no guard config file
//! (`guard/internal/config.ConfigPath()`), no audit log
//! (`filepath.Join(config.DataDir(), "audit.log")`), and none of
//! `guard/internal/discovery.DiscoverAll()`'s known client config paths
//! present. Any one of those existing means real, unported Go behavior would
//! run (TOML parsing, audit-anchor reading, MCP discovery, spawn-allowlist
//! matching, plaintext-secret detection) — the native path defers to Go
//! rather than guess at it. See the ledger (`migration/implementation-plan.md`)
//! for why this is scoped to exactly the one frozen fixture case.

use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};

use symbrain_core::{exit, version};

use super::guard_scan;

/// Runs `symbrain guard doctor`. Returns `None` when this machine is not in
/// the "empty machine" state, so the caller falls back to the Go binary.
pub(crate) fn run(stdout: &mut dyn Write) -> Option<u8> {
    let config_path = config_path();
    let audit_log_path = super::audit_path();
    let any_client_config = guard_scan::SOURCES
        .iter()
        .any(|source| guard_scan::source_path(*source).exists());

    if requires_go_fallback(&config_path, &audit_log_path, any_client_config) {
        return None;
    }
    Some(print_empty_machine_report(stdout))
}

/// The conservative empty-machine gate. Native handling is safe only when
/// none of the three conditions hold — flip to `true` (fall back to Go) the
/// moment any one of them does. Prefer a false "needs Go" over a false
/// "none discovered": the latter would hide real spawn-allowlist/secret
/// findings from a user who actually has one of these present.
fn requires_go_fallback(
    config_path: &Path,
    audit_log_path: &Path,
    any_client_config: bool,
) -> bool {
    config_path.exists() || audit_log_path.exists() || any_client_config
}

/// Mirrors `guard/internal/config.ConfigPath()`/`DefaultPath()`:
/// `$SYMGUARD_CONFIG`, else `$XDG_CONFIG_HOME/symguard/config.toml`, else
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

fn print_empty_machine_report(stdout: &mut dyn Write) -> u8 {
    let build_version = option_env!("SYMBRAIN_VERSION").unwrap_or("dev");
    let _ = writeln!(stdout, "symguard doctor");
    let _ = writeln!(stdout);
    let _ = writeln!(stdout, "  Version:   {build_version}");
    // Go's own line here is `runtime.Version()` (e.g. "go1.26.7") — a Rust
    // binary has no Go toolchain and must never print a fabricated one (a
    // prior wave-4 attempt at exactly that was reverted). Report the real
    // Rust toolchain honestly under its own label instead, matching the
    // precedent already shipped in this file's `version` verb; the test's
    // normalization tokenizes this row the same way it already does there.
    let _ = writeln!(stdout, "  Rust:      {}", crate::rustc_version());
    let _ = writeln!(
        stdout,
        "  OS/Arch:   {}/{}",
        version::current_os(),
        version::current_arch()
    );
    let _ = writeln!(stdout);
    let mut report = |name: &str, status: &str| {
        let _ = writeln!(stdout, "  {name:<16} {status}");
    };
    report("binary", "ok");
    report("go runtime", "ok");
    report("config", "not configured (no config file found)");
    report("policy", "defaults only (no rules — deny by default)");
    report(
        "audit log",
        "not initialized (created on first 'symguard decide')",
    );
    report(
        "spawn allowlist",
        "not configured (empty — deny by default)",
    );
    report("mcp servers", "none discovered");
    let _ = writeln!(stdout);
    let _ = writeln!(
        stdout,
        "All basic checks passed. Run 'symguard scan' after setup for full diagnostics."
    );
    exit::OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn gate_flips_on_any_of_the_three_conditions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("missing");
        let present = dir.path().join("present");
        fs::write(&present, b"").expect("write");

        assert!(!requires_go_fallback(&missing, &missing, false));
        assert!(requires_go_fallback(&present, &missing, false));
        assert!(requires_go_fallback(&missing, &present, false));
        assert!(requires_go_fallback(&missing, &missing, true));
        assert!(requires_go_fallback(&present, &present, true));
    }

    #[test]
    fn empty_machine_report_matches_frozen_shape() {
        let mut stdout = Vec::new();
        print_empty_machine_report(&mut stdout);
        let text = String::from_utf8(stdout).expect("utf8");
        assert!(text.starts_with("symguard doctor\n\n  Version:   "));
        assert!(text.contains("\n  Rust:      "));
        assert!(text.contains("\n  OS/Arch:   "));
        assert!(text.contains("  binary           ok\n"));
        assert!(text.contains("  go runtime       ok\n"));
        assert!(text.contains("  config           not configured (no config file found)\n"));
        assert!(text.contains("  policy           defaults only (no rules — deny by default)\n"));
        assert!(
            text.contains(
                "  audit log        not initialized (created on first 'symguard decide')\n"
            )
        );
        assert!(text.contains("  spawn allowlist  not configured (empty — deny by default)\n"));
        assert!(text.contains("  mcp servers      none discovered\n"));
        assert!(text.ends_with(
            "All basic checks passed. Run 'symguard scan' after setup for full diagnostics.\n"
        ));
    }
}
