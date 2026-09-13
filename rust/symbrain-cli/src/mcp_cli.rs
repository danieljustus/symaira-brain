//! Native `mcp`/`serve` command wiring for the Rust gateway.
//!
//! Only backends with a real native implementation are connected here. Skills
//! remain fail-closed until their native handlers are available; Memory and
//! Activity are served by the embedded gateway store.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use symbrain_audit::{Config as AuditConfig, Logger};
use symbrain_broker::{Config as BrokerConfig, ManagedServer};
use symbrain_core::exit;
use symbrain_gateway::{Gateway, GatewayBackend};
use symbrain_policy::{Profile, SERVER_OPERATE, SERVER_SCOPE, VAULT_MODE_OFF, load, load_file};
use toml_edit::{DocumentMut, Item, Value};

const VAULT_BINARY: &str = "symvault";
const VAULT_BINARY_ENV: &str = "SYMBRAIN_SERVERS_VAULT_BINARY_PATH";
const OPERATE_BINARY: &str = "symoperate";
const OPERATE_BINARY_ENV: &str = "SYMBRAIN_SERVERS_OPERATE_BINARY_PATH";
const SCOPE_BINARY: &str = "symscope";
const SCOPE_BINARY_ENV: &str = "SYMBRAIN_SERVERS_SCOPE_BINARY_PATH";
const MCP_USAGE: &str = "Usage of mcp:\n  -profile string\n    profile name to serve (required unless --profile-file is given)\n  -profile-file string\n    load the profile from this TOML file instead of the profiles directory\n  -vault-agent string\n    vault agent name for --stdio mode\n";

#[derive(Debug, Default)]
struct McpArgs {
    profile: Option<String>,
    profile_file: Option<PathBuf>,
    vault_agent: Option<String>,
}

/// Runs the native MCP gateway over the process stdio streams.
///
/// # Errors
/// Returns a process-style exit code. Configuration and profile failures use
/// exit code 2; protocol failures use exit code 1.
pub(crate) fn run(args: &[OsString], stderr: &mut dyn Write) -> u8 {
    let parsed = match parse_args(args, stderr) {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };

    let profile = match resolve_profile(&parsed) {
        Ok(profile) => profile,
        Err(error) => {
            let _ = writeln!(stderr, "symbrain mcp: {error}");
            return exit::NO_INPUT;
        }
    };
    if !check_embedded_handlers(&profile, stderr) {
        return exit::NO_INPUT;
    }

    let mut managed = Vec::new();
    let mut backends: BTreeMap<String, Arc<dyn GatewayBackend>> = BTreeMap::new();
    build_backends(
        &profile,
        parsed.vault_agent.as_deref(),
        stderr,
        &mut managed,
        &mut backends,
    );

    let audit = if profile.audit.enabled {
        match Logger::open(
            &profile.name,
            AuditConfig {
                enabled: true,
                verbose: profile.audit.verbose,
            },
        ) {
            Ok(logger) => Some(Arc::new(logger)),
            Err(error) => {
                let _ = writeln!(stderr, "symbrain mcp: failed to open audit log: {error}");
                None
            }
        }
    } else {
        None
    };

    let gateway = match Gateway::new(
        profile,
        backends,
        option_env!("SYMBRAIN_VERSION").unwrap_or("dev"),
    )
    .map(|gateway| gateway.with_audit(audit))
    {
        Ok(gateway) => gateway,
        Err(error) => {
            shutdown_all(&managed);
            let _ = writeln!(stderr, "symbrain mcp: build gateway: {error}");
            return exit::NO_INPUT;
        }
    };

    let cancelled = match install_signal_flag() {
        Ok(flag) => flag,
        Err(error) => {
            shutdown_all(&managed);
            let _ = writeln!(stderr, "symbrain mcp: install signal handlers: {error}");
            return exit::GENERIC;
        }
    };
    if cancelled.load(Ordering::Acquire) {
        shutdown_all(&managed);
        return exit::OK;
    }

    // Keep the blocking stdin read off the signal-monitoring thread. On Unix,
    // a signal can arrive while the MCP peer has the pipe open and is not
    // sending a frame; the monitor still shuts down every child process group.
    let worker_cancelled = Arc::clone(&cancelled);
    let worker = thread::spawn(move || {
        let stdin = io::stdin();
        let mut stdout = io::stdout();
        symbrain_mcp::Server::new(&gateway).serve_io_with_cancel(
            worker_cancelled.as_ref(),
            stdin.lock(),
            &mut stdout,
        )
    });

    loop {
        if worker.is_finished() {
            let result = worker.join();
            shutdown_all(&managed);
            return match result {
                Ok(Ok(())) => exit::OK,
                Ok(Err(error)) => {
                    let _ = writeln!(stderr, "symbrain mcp: {error}");
                    exit::GENERIC
                }
                Err(_) => {
                    let _ = writeln!(stderr, "symbrain mcp: gateway worker panicked");
                    exit::GENERIC
                }
            };
        }
        if cancelled.load(Ordering::Acquire) {
            shutdown_all(&managed);
            // The worker may still be blocked in stdin. It owns only process
            // stdio and will be torn down with this CLI process; child groups
            // have already been terminated above.
            return exit::OK;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Runs the deprecated `serve` spelling without changing the MCP transport.
pub(crate) fn run_serve(args: &[OsString], stderr: &mut dyn Write) -> u8 {
    let _ = writeln!(
        stderr,
        "symbrain: 'serve' is deprecated; use 'symbrain mcp' instead (serve will be removed in a future release)"
    );
    run(args, stderr)
}

fn parse_args(args: &[OsString], stderr: &mut dyn Write) -> Result<McpArgs, u8> {
    let args = crate::normalize_flags(args);
    let mut parsed = McpArgs::default();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let text = arg.to_string_lossy();
        if text == "--" {
            if let Some(unexpected) = args.get(i + 1) {
                let _ = writeln!(
                    stderr,
                    "symbrain mcp: unexpected argument {:?}",
                    unexpected.to_string_lossy()
                );
                return Err(exit::NO_INPUT);
            }
            break;
        }
        if let Some(value) = inline_value(&text, "profile") {
            parsed.profile = Some(value);
        } else if let Some(value) = inline_value(&text, "profile-file") {
            parsed.profile_file = Some(PathBuf::from(value));
        } else if let Some(value) = inline_value(&text, "vault-agent") {
            parsed.vault_agent = Some(value);
        } else if matches!(text.as_ref(), "-profile" | "--profile") {
            let Some(value) = args.get(i + 1) else {
                return missing_value(stderr, "profile");
            };
            parsed.profile = Some(value.to_string_lossy().into_owned());
            i += 1;
        } else if matches!(text.as_ref(), "-profile-file" | "--profile-file") {
            let Some(value) = args.get(i + 1) else {
                return missing_value(stderr, "profile-file");
            };
            parsed.profile_file = Some(PathBuf::from(value));
            i += 1;
        } else if matches!(text.as_ref(), "-vault-agent" | "--vault-agent") {
            let Some(value) = args.get(i + 1) else {
                return missing_value(stderr, "vault-agent");
            };
            parsed.vault_agent = Some(value.to_string_lossy().into_owned());
            i += 1;
        } else if matches!(text.as_ref(), "-h" | "-help" | "--help") {
            let _ = write!(stderr, "{MCP_USAGE}");
            return Err(exit::NO_INPUT);
        } else if text.starts_with('-') {
            let _ = writeln!(
                stderr,
                "flag provided but not defined: -{}",
                text.trim_start_matches('-')
            );
            let _ = write!(stderr, "{MCP_USAGE}");
            return Err(exit::NO_INPUT);
        } else {
            let _ = writeln!(stderr, "symbrain mcp: unexpected argument {text:?}");
            return Err(exit::NO_INPUT);
        }
        i += 1;
    }
    if parsed.profile.is_none() && parsed.profile_file.is_none() {
        let _ = writeln!(
            stderr,
            "symbrain mcp: --profile is required (or --profile-file <path>)"
        );
        return Err(exit::NO_INPUT);
    }
    if parsed.profile.is_some() && parsed.profile_file.is_some() {
        let _ = writeln!(
            stderr,
            "symbrain mcp: --profile and --profile-file are mutually exclusive"
        );
        return Err(exit::NO_INPUT);
    }
    Ok(parsed)
}

fn inline_value(arg: &str, name: &str) -> Option<String> {
    arg.strip_prefix(&format!("-{name}="))
        .or_else(|| arg.strip_prefix(&format!("--{name}=")))
        .map(ToOwned::to_owned)
}

fn missing_value(stderr: &mut dyn Write, name: &str) -> Result<McpArgs, u8> {
    let _ = writeln!(stderr, "flag needs an argument: -{name}");
    let _ = write!(stderr, "{MCP_USAGE}");
    Err(exit::NO_INPUT)
}

fn resolve_profile(args: &McpArgs) -> Result<Profile, symbrain_policy::ProfileError> {
    if let Some(path) = &args.profile_file {
        load_file(path)
    } else {
        load(args.profile.as_deref().unwrap_or_default())
    }
}

fn check_embedded_handlers(profile: &Profile, stderr: &mut dyn Write) -> bool {
    let blocked = ["skills"]
        .into_iter()
        .filter(|alias| profile.server(alias).enabled)
        .collect::<Vec<_>>();
    if blocked.is_empty() {
        return true;
    }
    let names = blocked.join(", ");
    let _ = writeln!(
        stderr,
        "symbrain mcp: blocked profile: native embedded handlers are not ported for {names}; disable these servers until their native handlers land (no Go fallback)"
    );
    false
}

fn build_backends(
    profile: &Profile,
    vault_agent: Option<&str>,
    stderr: &mut dyn Write,
    managed: &mut Vec<Arc<ManagedServer>>,
    backends: &mut BTreeMap<String, Arc<dyn GatewayBackend>>,
) {
    let vault = profile.server("vault");
    if vault.enabled && vault.mode != VAULT_MODE_OFF {
        let override_path = env::var(VAULT_BINARY_ENV)
            .ok()
            .filter(|value| !value.is_empty())
            .or_else(configured_vault_path);
        match symbrain_broker::discover(VAULT_BINARY, override_path.as_deref().unwrap_or_default())
        {
            Ok(path) => {
                let mut args = vec!["serve".to_string()];
                if let Some(agent) = vault_agent {
                    args.push("--stdio".to_string());
                    args.push("--agent".to_string());
                    args.push(agent.to_string());
                }
                args.push("--allow-locked".to_string());
                insert_managed("vault", path, args, managed, backends);
            }
            Err(error) => {
                let _ = writeln!(stderr, "symbrain mcp: vault: {error}");
            }
        }
    }

    for alias in profile.server_aliases() {
        if symbrain_policy::is_core_alias(&alias) {
            continue;
        }
        let config = profile.server(&alias);
        if !config.enabled {
            continue;
        }
        // Optional Cockpit modules are independently gated by global config
        // and profile opt-in. They are never treated as foreign commands,
        // and their public stdio entrypoints are the unified dispatcher
        // subcommands verified in symaira-cockpit's source.
        if alias == SERVER_OPERATE || alias == SERVER_SCOPE {
            if !optional_module_enabled(&alias) {
                continue;
            }
            let (binary, binary_env, command) = if alias == SERVER_OPERATE {
                (OPERATE_BINARY, OPERATE_BINARY_ENV, "operate")
            } else {
                (SCOPE_BINARY, SCOPE_BINARY_ENV, "scope")
            };
            let override_path = env::var(binary_env)
                .ok()
                .filter(|value| !value.is_empty())
                .or_else(|| configured_module_path(&alias));
            let discovered = if let Some(path) = override_path.as_deref() {
                symbrain_broker::discover(binary, path).map(|path| (path, false))
            } else {
                symbrain_broker::discover(binary, "")
                    .map(|path| (path, false))
                    .or_else(|_| {
                        symbrain_broker::discover("symcockpit", "").map(|path| (path, true))
                    })
            };
            match discovered {
                Ok((path, legacy)) => {
                    let args = if legacy {
                        vec![command.to_string(), "serve".to_string()]
                    } else {
                        vec!["serve".to_string()]
                    };
                    insert_managed(&alias, path, args, managed, backends);
                }
                Err(error) => {
                    let _ = writeln!(stderr, "symbrain mcp: {alias}: {error}");
                }
            }
            continue;
        }
        if !config.url.is_empty() && config.command.is_empty() {
            let _ = writeln!(
                stderr,
                "symbrain mcp: {alias}: URL MCP transport is not ported; skipping (no Go fallback)"
            );
            continue;
        }
        if config.command.is_empty() {
            continue;
        }
        let command_path = Path::new(&config.command);
        let (binary_name, override_path) = if command_path.components().count() > 1 {
            (
                command_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(&config.command),
                config.command.as_str(),
            )
        } else {
            (config.command.as_str(), "")
        };
        match symbrain_broker::discover(binary_name, override_path) {
            Ok(path) => insert_managed(&alias, path, config.args.clone(), managed, backends),
            Err(error) => {
                let _ = writeln!(stderr, "symbrain mcp: {alias}: {error}");
            }
        }
    }
}

fn optional_module_enabled(alias: &str) -> bool {
    let key = match alias {
        SERVER_OPERATE => "operate",
        SERVER_SCOPE => "scope",
        _ => return false,
    };
    if let Some(value) = std::env::var_os(format!("SYMBRAIN_MODULES_{}", key.to_uppercase())) {
        return matches!(
            value.to_string_lossy().as_ref(),
            "1" | "t" | "T" | "TRUE" | "True" | "true"
        );
    }
    let path = symbrain_core::xdg::config_path();
    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(document) = contents.parse::<DocumentMut>() else {
        return false;
    };
    document
        .get("modules")
        .and_then(|item| item.get(key))
        .and_then(Item::as_bool)
        .unwrap_or(false)
}

fn insert_managed(
    alias: &str,
    path: String,
    args: Vec<String>,
    managed: &mut Vec<Arc<ManagedServer>>,
    backends: &mut BTreeMap<String, Arc<dyn GatewayBackend>>,
) {
    let server = Arc::new(ManagedServer::new(BrokerConfig {
        name: alias.to_string(),
        binary_path: path,
        args,
        init_timeout: Duration::from_secs(10),
        call_timeout: Duration::from_secs(30),
        max_restarts: 3,
        backoff_base: Duration::from_secs(1),
        shutdown_timeout: Duration::from_secs(5),
        env: None,
        capture_stderr: false,
    }));
    backends.insert(
        alias.to_string(),
        Arc::clone(&server) as Arc<dyn GatewayBackend>,
    );
    managed.push(server);
}

fn shutdown_all(servers: &[Arc<ManagedServer>]) {
    for server in servers {
        server.shutdown();
    }
}

fn configured_vault_path() -> Option<String> {
    let contents = std::fs::read_to_string(symbrain_core::xdg::config_path()).ok()?;
    let document: DocumentMut = contents.parse().ok()?;
    let servers = document.get("servers")?.as_table_like()?;
    let vault = servers.get("vault")?.as_table_like()?;
    match vault.get("binary_path")? {
        Item::Value(Value::String(value)) if !value.value().is_empty() => {
            Some(value.value().clone())
        }
        _ => None,
    }
}

fn configured_module_path(alias: &str) -> Option<String> {
    let contents = std::fs::read_to_string(symbrain_core::xdg::config_path()).ok()?;
    let document: DocumentMut = contents.parse().ok()?;
    configured_module_path_from(&document, alias)
}

fn configured_module_path_from(document: &DocumentMut, alias: &str) -> Option<String> {
    let servers = document.get("servers")?.as_table_like()?;
    let server = servers.get(alias)?.as_table_like()?;
    match server.get("binary_path")? {
        Item::Value(Value::String(value)) if !value.value().is_empty() => {
            Some(value.value().clone())
        }
        _ => None,
    }
}

fn install_signal_flag() -> io::Result<Arc<AtomicBool>> {
    let cancelled = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&cancelled))?;
    #[cfg(unix)]
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&cancelled))?;
    Ok(cancelled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_flags_accept_both_dash_forms() {
        assert_eq!(
            inline_value("-profile=one", "profile"),
            Some("one".to_string())
        );
        assert_eq!(
            inline_value("--profile=one", "profile"),
            Some("one".to_string())
        );
        assert_eq!(
            inline_value("-profile-file=room.toml", "profile-file"),
            Some("room.toml".to_string())
        );
    }

    #[test]
    fn configured_module_path_reads_server_override() {
        let document: DocumentMut = "[servers.operate]\nbinary_path = \"/opt/symoperate\"\n"
            .parse()
            .expect("valid config");
        assert_eq!(
            configured_module_path_from(&document, SERVER_OPERATE),
            Some("/opt/symoperate".to_string())
        );
        let empty: DocumentMut = "[servers.operate]\nbinary_path = \"\"\n"
            .parse()
            .expect("valid config");
        assert_eq!(configured_module_path_from(&empty, SERVER_OPERATE), None);
    }
}
