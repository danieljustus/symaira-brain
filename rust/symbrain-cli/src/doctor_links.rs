use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;
use std::time::Duration;

use symbrain_core::xdg;
use symbrain_policy::{self as policy, ServerConfig};
use toml_edit::DocumentMut;

use super::doctor_process::{run_process, which};
use super::doctor_types::{ForeignAccessRisk, HarnessCheck, LinkCheck, ProfileHandshake};

use symbrain_broker::{Client, Options};

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) fn check_handshakes(vault_agent: &str) -> Vec<ProfileHandshake> {
    let names = symbrain_policy::list_names().unwrap_or_default();
    let mut result = Vec::new();
    for name in names {
        let Ok(profile) = symbrain_policy::load(&name) else {
            continue;
        };
        let server = profile.server(policy::SERVER_VAULT);
        if !server.enabled {
            continue;
        }
        let path = match discover_vault() {
            Ok(path) => path,
            Err(error) => {
                result.push(ProfileHandshake {
                    profile: name,
                    server: "vault".to_string(),
                    protocol_version: String::new(),
                    tool_count: 0,
                    exposed: 0,
                    hidden: 0,
                    unknown: 0,
                    error,
                });
                continue;
            }
        };
        result.push(probe_handshake(&path, &name, vault_agent, &server));
    }
    result
}

fn discover_vault() -> Result<String, String> {
    let override_path = config_binary_override();
    symbrain_broker::discover("symvault", &override_path).map_err(|error| error.to_string())
}

fn config_binary_override() -> String {
    let path = xdg::config_path();
    let Ok(bytes) = fs::read_to_string(path) else {
        return String::new();
    };
    let Ok(doc) = bytes.parse::<DocumentMut>() else {
        return String::new();
    };
    doc.get("servers")
        .and_then(|v| v.as_table())
        .and_then(|v| v.get("vault"))
        .and_then(|v| v.as_table())
        .and_then(|v| v.get("binary_path"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

fn probe_handshake(
    path: &str,
    profile_name: &str,
    vault_agent: &str,
    server: &ServerConfig,
) -> ProfileHandshake {
    let mut result = ProfileHandshake {
        profile: profile_name.to_string(),
        server: "vault".to_string(),
        protocol_version: String::new(),
        tool_count: 0,
        exposed: 0,
        hidden: 0,
        unknown: 0,
        error: String::new(),
    };
    let client = match Client::spawn(
        path,
        Options {
            args: vec![
                "serve".to_string(),
                "--stdio".to_string(),
                "--agent".to_string(),
                vault_agent.to_string(),
                "--allow-locked".to_string(),
            ],
            env: None,
            capture_stderr: true,
        },
    ) {
        Ok(client) => client,
        Err(error) => {
            result.error = format!("spawn: {error}");
            return result;
        }
    };
    let init = match client.initialize(HANDSHAKE_TIMEOUT) {
        Ok(init) => init,
        Err(error) => {
            result.error = format!("initialize: {error}");
            return result;
        }
    };
    result.protocol_version = init.protocol_version;
    let tools = match client.list_tools(HANDSHAKE_TIMEOUT) {
        Ok(tools) => tools,
        Err(error) => {
            result.error = format!("tools/list: {error}");
            return result;
        }
    };
    result.tool_count = tools.len();
    let names = tools.into_iter().map(|tool| tool.name).collect::<Vec<_>>();
    match policy::evaluate("vault", server, &names) {
        Ok(report) => {
            result.exposed = report.exposed.len();
            result.hidden = report.hidden.len();
            result.unknown = report.unknown.len();
        }
        Err(error) => result.error = format!("policy: {error}"),
    }
    result
}

pub(super) fn check_links(harnesses: &[HarnessCheck]) -> Vec<LinkCheck> {
    let mut links = vec![check_vault_reachable()];
    let refs = configured_secret_refs();
    if refs.is_empty() {
        links.push(LinkCheck {
            name: "vault: secret reference resolves".to_string(),
            status: "unknown".to_string(),
            detail: "no configured secret reference found to test".to_string(),
            remedy: String::new(),
        });
    } else {
        for (source, value) in refs {
            links.push(resolve_secret_ref(&source, &value));
        }
    }
    let profiles = symbrain_policy::list_names().unwrap_or_default();
    let mut registered = BTreeMap::new();
    for harness in harnesses {
        if harness.installed && !harness.profile.is_empty() {
            registered.insert(harness.profile.clone(), true);
        }
    }
    for profile in profiles {
        if registered.contains_key(&profile) {
            links.push(LinkCheck {
                name: format!("profile {profile:?}: registered"),
                status: "pass".to_string(),
                detail: "bound to at least one harness".to_string(),
                remedy: String::new(),
            });
        } else {
            links.push(LinkCheck {
                name: format!("profile {profile:?}: registered"),
                status: "fail".to_string(),
                detail: "not bound to any harness config".to_string(),
                remedy: format!("run `symbrain install --harness <name> --profile {profile}`"),
            });
        }
    }
    links
}

pub(super) fn check_vault_reachable() -> LinkCheck {
    let name = "vault: reachable".to_string();
    let Some(path) = which("symvault") else {
        return LinkCheck {
            name,
            status: "unknown".to_string(),
            detail: "symvault not installed".to_string(),
            remedy: "run `symbrain setup` to install the managed cores".to_string(),
        };
    };
    match run_process(
        &path,
        &["get", "__symbrain_doctor_probe__", "--print"],
        PROBE_TIMEOUT,
    ) {
        Ok((status, _, _stderr)) if status.success() => LinkCheck {
            name,
            status: "pass".to_string(),
            detail: "reachable and unlocked".to_string(),
            remedy: String::new(),
        },
        Ok((status, _, stderr)) => classify_vault_failure(name, status, &stderr),
        Err(error) if error.contains("timed out") => LinkCheck {
            name,
            status: "unknown".to_string(),
            detail: "probe timed out".to_string(),
            remedy: String::new(),
        },
        Err(error) => LinkCheck {
            name,
            status: "fail".to_string(),
            detail: format!("symvault probe failed: {error}"),
            remedy: "run `symvault doctor` to diagnose".to_string(),
        },
    }
}

pub(super) fn classify_vault_failure(
    name: String,
    status: std::process::ExitStatus,
    stderr: &[u8],
) -> LinkCheck {
    let message = String::from_utf8_lossy(stderr).to_lowercase();
    if message.contains("not found")
        || message.contains("no such")
        || message.contains("no matching")
    {
        LinkCheck {
            name,
            status: "pass".to_string(),
            detail: "reachable and unlocked".to_string(),
            remedy: String::new(),
        }
    } else if message.contains("locked")
        || message.contains("passphrase")
        || message.contains("unlock")
    {
        LinkCheck {
            name,
            status: "pass".to_string(),
            detail: "reachable but locked".to_string(),
            remedy: "run `symvault unlock` to authenticate".to_string(),
        }
    } else {
        let detail = status
            .code()
            .map_or_else(|| status.to_string(), |code| format!("exit status {code}"));
        LinkCheck {
            name,
            status: "fail".to_string(),
            detail: format!("symvault probe failed: {detail}"),
            remedy: "run `symvault doctor` to diagnose".to_string(),
        }
    }
}

pub(super) fn configured_secret_refs() -> Vec<(String, String)> {
    let mut refs = Vec::new();
    if let Ok(contents) = fs::read_to_string(xdg::config_path())
        && let Ok(doc) = contents.parse::<DocumentMut>()
        && let Some(secret) = doc
            .get("jwt")
            .and_then(|value| value.as_table())
            .and_then(|table| table.get("secret"))
            .and_then(|value| value.as_str())
        && is_secret_reference(secret)
    {
        refs.push(("memory: jwt.secret".to_string(), secret.to_string()));
    }
    for name in symbrain_policy::list_names().unwrap_or_default() {
        let Ok(profile) = symbrain_policy::load(&name) else {
            continue;
        };
        for (alias, server) in profile.servers {
            for (index, arg) in server.args.iter().enumerate() {
                if is_secret_reference(arg) {
                    refs.push((format!("profile {name}: {alias} arg[{index}]"), arg.clone()));
                }
            }
        }
    }
    refs.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    refs
}

pub(super) fn is_secret_reference(value: &str) -> bool {
    ["symvault://", "vault://", "env://", "keychain://"]
        .iter()
        .any(|prefix| value.starts_with(prefix))
}

pub(super) fn resolve_secret_ref(source: &str, value: &str) -> LinkCheck {
    let name = format!("vault: secret reference resolves ({source})");
    if let Some(variable) = value.strip_prefix("env://") {
        return if env::var(variable).is_ok_and(|value| !value.is_empty()) {
            LinkCheck {
                name,
                status: "pass".to_string(),
                detail: "resolved successfully".to_string(),
                remedy: String::new(),
            }
        } else {
            LinkCheck {
                name,
                status: "fail".to_string(),
                detail: "resolution failed".to_string(),
                remedy: "check the referenced environment variable is set".to_string(),
            }
        };
    }
    if let Some(reference) = value.strip_prefix("keychain://") {
        let mut parts = reference.splitn(2, '/');
        let service = parts.next().unwrap_or_default();
        let account = parts.next().unwrap_or_default();
        let result = run_process(
            Path::new("/usr/bin/security"),
            &["find-generic-password", "-s", service, "-a", account, "-w"],
            PROBE_TIMEOUT,
        );
        return match result {
            Ok((status, _, _)) if status.success() => LinkCheck {
                name,
                status: "pass".to_string(),
                detail: "resolved successfully".to_string(),
                remedy: String::new(),
            },
            _ => LinkCheck {
                name,
                status: "fail".to_string(),
                detail: "resolution failed".to_string(),
                remedy: "check the referenced keychain item exists".to_string(),
            },
        };
    }
    let Some(path) = which("symvault") else {
        return LinkCheck {
            name,
            status: "fail".to_string(),
            detail: "resolution failed".to_string(),
            remedy: "check the reference path exists in the vault and the vault is unlocked"
                .to_string(),
        };
    };
    match run_process(
        &path,
        &[
            "get",
            value
                .trim_start_matches("symvault://")
                .trim_start_matches("vault://"),
            "--print",
        ],
        PROBE_TIMEOUT,
    ) {
        Ok((status, _, _)) if status.success() => LinkCheck {
            name,
            status: "pass".to_string(),
            detail: "resolved successfully".to_string(),
            remedy: String::new(),
        },
        _ => LinkCheck {
            name,
            status: "fail".to_string(),
            detail: "resolution failed".to_string(),
            remedy: "check the reference path exists in the vault and the vault is unlocked"
                .to_string(),
        },
    }
}

pub(super) fn check_foreign_access_risks() -> Vec<ForeignAccessRisk> {
    let mut result = Vec::new();
    for name in symbrain_policy::list_names().unwrap_or_default() {
        let Ok(profile) = symbrain_policy::load(&name) else {
            continue;
        };
        for (alias, server) in profile.servers {
            if policy::is_core_alias(&alias)
                || !server.enabled
                || server.access != policy::FOREIGN_ACCESS_READ
                || !server.tools_read.is_empty()
            {
                continue;
            }
            result.push(ForeignAccessRisk { profile: name.clone(), server: alias, detail: "access=read with no tools_read override — exposes nothing unless the upstream server sets readOnlyHint on its tools".to_string() });
        }
    }
    result.sort_by(|a, b| a.profile.cmp(&b.profile).then(a.server.cmp(&b.server)));
    result
}
