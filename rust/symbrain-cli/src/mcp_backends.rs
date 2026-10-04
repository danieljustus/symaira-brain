//! One resolved Brain value controls optional children before any spawn.
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use symbrain_broker::{Config as BrokerConfig, ManagedServer};
use symbrain_core::config::resolved::BrainConfig;
use symbrain_gateway::GatewayBackend;
use symbrain_policy::{Profile, SERVER_OPERATE, SERVER_SCOPE, VAULT_MODE_OFF};

pub(super) fn build_backends(
    profile: &Profile,
    resolved: &BrainConfig,
    vault_agent: Option<&str>,
    stderr: &mut dyn Write,
    managed: &mut Vec<Arc<ManagedServer>>,
    backends: &mut BTreeMap<String, Arc<dyn GatewayBackend>>,
) {
    let vault = profile.server("vault");
    if vault.enabled && vault.mode != VAULT_MODE_OFF {
        let override_path = symbrain_core::go_path::from_bytes(resolved.servers.vault.as_ref());
        match symbrain_broker::discover_path("symvault", &override_path) {
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
                write_discovery_error(stderr, "vault", &error);
            }
        }
    }

    for alias in profile.server_aliases() {
        if symbrain_policy::is_core_alias(&alias)
            && alias != SERVER_OPERATE
            && alias != SERVER_SCOPE
        {
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
            let (enabled, override_value, binary, command) = if alias == SERVER_OPERATE {
                (
                    resolved.modules.operate,
                    &resolved.servers.operate,
                    "symoperate",
                    "operate",
                )
            } else {
                (
                    resolved.modules.scope,
                    &resolved.servers.scope,
                    "symscope",
                    "scope",
                )
            };
            if !enabled {
                continue;
            }
            let override_path = symbrain_core::go_path::from_bytes(override_value.as_ref());
            let discovered = if !override_path.is_empty() {
                symbrain_broker::discover_path(binary, &override_path).map(|path| (path, false))
            } else {
                symbrain_broker::discover_path(binary, std::ffi::OsStr::new(""))
                    .map(|path| (path, false))
                    .or_else(|_| {
                        symbrain_broker::discover_path("symcockpit", std::ffi::OsStr::new(""))
                            .map(|path| (path, true))
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
                    write_discovery_error(stderr, &alias, &error);
                }
            }
            continue;
        }
        if !config.url.is_empty() && config.command.is_empty() {
            let _ = writeln!(
                stderr,
                "symbrain mcp: {alias}: url-only foreign server (no stdio transport yet); skipping"
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
        match symbrain_broker::discover_path(binary_name, std::ffi::OsStr::new(override_path)) {
            Ok(path) => insert_managed(&alias, path, config.args.clone(), managed, backends),
            Err(error) => {
                write_discovery_error(stderr, &alias, &error);
            }
        }
    }
}

fn insert_managed(
    alias: &str,
    path: PathBuf,
    args: Vec<String>,
    managed: &mut Vec<Arc<ManagedServer>>,
    backends: &mut BTreeMap<String, Arc<dyn GatewayBackend>>,
) {
    let server = Arc::new(ManagedServer::new_path(
        BrokerConfig {
            name: alias.to_string(),
            binary_path: String::new(),
            args,
            init_timeout: Duration::from_secs(10),
            call_timeout: Duration::from_secs(30),
            max_restarts: 3,
            backoff_base: Duration::from_secs(1),
            shutdown_timeout: Duration::from_secs(5),
            env: None,
            capture_stderr: false,
        },
        path,
    ));
    backends.insert(
        alias.to_string(),
        Arc::clone(&server) as Arc<dyn GatewayBackend>,
    );
    managed.push(server);
}

pub(super) fn shutdown_all(servers: &[Arc<ManagedServer>]) {
    for server in servers {
        server.shutdown();
    }
}

fn write_discovery_error(
    stderr: &mut dyn Write,
    alias: &str,
    error: &symbrain_broker::PathDiscoveryError,
) {
    let mut line = format!("symbrain mcp: {alias}: ").into_bytes();
    line.extend_from_slice(error.text.as_ref());
    line.push(b'\n');
    let _ = stderr.write_all(&line);
}
