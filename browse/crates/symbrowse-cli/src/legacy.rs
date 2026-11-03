//! Runners carried over from the first Rust CLI (daemon run, mcp serve, flow,
//! config show, state key init). They print on the process streams.



use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
    time::Duration,
};

use serde::Serialize;
use symbrowse_core::{
    batch::{self, ItemOutput},
    config::{FlagOverrides, LoadContext, load, render_show_text, render_show_yaml, show_fields},
    error::ErrorCode,
    flows,
    key_resolver::{KeyInitResult, KeyResolver},
    key_sources::SystemKeySources,
    output::{Envelope, Format},
};
use symbrowse_daemon::{
    Client, ClientOptions, Frame, PolicyStatus, Server, ServerOptions, SessionSpec,
    codes as daemon_codes, default_socket_path, dispatch_once,
};
use symbrowse_mcp::{ServeOptions, registry, serve_stdio};
use symbrowse_protocol::{render_root_version, render_version_json, render_version_text};

pub(crate) const VERSION: &str = match option_env!("SYMBROWSE_VERSION") {
    Some(version) => version,
    None => "dev",
};


#[derive(Serialize)]
pub(crate) struct ConfigSuccess<T> {
    success: bool,
    data: T,
}

#[derive(Serialize)]
pub(crate) struct ConfigData {
    fields: std::collections::BTreeMap<String, symbrowse_core::config::Field>,
}

#[derive(Serialize)]
pub(crate) struct BatchSuccess<'a> {
    pub(crate) success: bool,
    pub(crate) data: &'a batch::Report,
}

#[derive(Serialize)]
pub(crate) struct StateSuccess<'a> {
    success: bool,
    data: &'a KeyInitResult,
}

pub(crate) const MCP_PROFILE_LIST: &str = "core     14 tools  Page interaction and reading: open, snapshot, click, fill, type, press, wait, read, get, find. The default profile.\n           tools: open, snapshot, click, fill, type, press, wait, read, get, find, fetch_url, fetch_batch, cache_get, wayback_snapshots\nnav       3 tools  History navigation: back, forward, reload.\n           tools: back, forward, reload\nstate     0 tools  Sessions, cookies, storage, state save/load, auth login. Tools land with the state milestone (v0.4.0).\nnetwork   0 tools  Routing, mocking, request inspection, HAR, headers, offline. Tools land with the network milestone (v1.0.0).\ndebug     0 tools  Console, errors, eval, a11y, diff, doctor. Tools land with the reach milestones (v1.0.0).\nflows     0 tools  Flow list/run/record. Tools land with the flows milestone (v0.6.0).\n";

pub(crate) fn render_dispatch_error(format: Format, code: &str, message: String) -> ExitCode {
    let mapped = match code {
        daemon_codes::MALFORMED_REQUEST => ErrorCode::MalformedRequest,
        daemon_codes::UNKNOWN_COMMAND => ErrorCode::UnknownCommand,
        daemon_codes::OPERATION_TIMEOUT => ErrorCode::OperationTimeout,
        daemon_codes::PEER_DENIED => ErrorCode::PeerDenied,
        daemon_codes::DAEMON_UNAVAILABLE => ErrorCode::DaemonUnavailable,
        daemon_codes::INVALID_SESSION => ErrorCode::InvalidSession,
        symbrowse_core::config::TRANSPORT_RETIRED => ErrorCode::Config,
        _ => ErrorCode::OperationFailed,
    };
    match Envelope::failure(mapped, message).render(format) {
        Ok(output) => {
            let _ = io::stdout().write_all(output.as_bytes());
        }
        Err(_) => {
            let _ = writeln!(io::stderr(), "dispatch failed");
        }
    }
    ExitCode::from(mapped.exit_code())
}


pub(crate) fn run_flow_list(format: Format) -> ExitCode {
    let mut groups = Vec::new();
    let project = PathBuf::from(".symbrowse").join("flows");
    if let Ok(found) = flows::discover_directory(&project, "project") {
        groups.push(found);
    }
    if let Ok(home) = std::env::var("HOME") {
        let global = PathBuf::from(home)
            .join(".config")
            .join("symbrowse")
            .join("flows");
        if let Ok(found) = flows::discover_directory(&global, "global") {
            groups.push(found);
        }
    }
    let data = serde_json::json!({"flows": flows::merge_discovered(groups)});
    match Envelope::ok(data, Vec::new()).render(format) {
        Ok(output) => write_stdout(&output),
        Err(error) => {
            render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error.to_string())
        }
    }
}

pub(crate) fn run_flow_validate(path: PathBuf, format: Format) -> ExitCode {
    let source = match fs::read(&path) {
        Ok(source) => source,
        Err(error) => {
            return render_dispatch_error(
                format,
                "not_found",
                format!("read flow {}: {error}", path.display()),
            );
        }
    };
    match flows::parse(&source, path.to_string_lossy().to_string()) {
        Ok(flow) => {
            let data = serde_json::json!({
                "name": flow.name,
                "version": flow.version,
                "domains": flow.domains,
                "steps": flow.steps.len(),
                "outputs": flow.outputs.len(),
                "valid": true,
            });
            match Envelope::ok(data, Vec::new()).render(format) {
                Ok(output) => write_stdout(&output),
                Err(error) => {
                    render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error.to_string())
                }
            }
        }
        Err(error) => render_dispatch_error(format, "malformed_request", error.to_string()),
    }
}

pub(crate) fn run_mcp(
    session: String,
    profiles: String,
    allow_private: bool,
    engine: Option<String>,
) -> ExitCode {
    if let Err(error) = registry::validate_profile_selection(&profiles) {
        let _ = writeln!(io::stderr(), "{error}");
        return ExitCode::from(1);
    }
    let context = match LoadContext::from_process(FlagOverrides::default()) {
        Ok(context) => context,
        Err(error) => {
            let _ = writeln!(io::stderr(), "{error}");
            return ExitCode::from(1);
        }
    };
    let config = match load(&context) {
        Ok(result) => result.config,
        Err(error) => {
            if is_transport_retired(&error.to_string()) {
                // MCP stdout carries JSON-RPC only: report on stderr.
                return write_transport_retired(Format::Text, &error.to_string());
            }
            let _ = writeln!(io::stderr(), "{error}");
            return ExitCode::from(1);
        }
    };
    let daemon_log_path = config.daemon_log.clone();
    let engine = engine.unwrap_or(config.engine);
    let allow_private = allow_private || config.allow_private;
    if let Err(message) = validate_engine(&engine) {
        let _ = writeln!(io::stderr(), "{message}");
        return ExitCode::from(1);
    }
    let stdin = io::stdin();
    let stdout = io::stdout();
    match serve_stdio(
        stdin.lock(),
        stdout.lock(),
        ServeOptions {
            version: VERSION.to_owned(),
            session,
            profiles,
            executable: std::env::current_exe()
                .ok()
                .and_then(|path| path.into_os_string().into_string().ok())
                .unwrap_or_else(|| "symbrowse".to_owned()),
            allow_private,
            engine: Some(engine),
            daemon_log_path: Some(daemon_log_path),
            endpoint: None,
        },
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr(), "mcp server: {error}");
            ExitCode::from(1)
        }
    }
}

pub(crate) fn validate_engine(value: &str) -> Result<(), String> {
    if matches!(value, "chrome" | "static" | "safari-attach" | "safari-bidi") {
        Ok(())
    } else {
        Err(format!(
            "invalid engine {value:?}: use one of chrome, static, safari-attach, safari-bidi"
        ))
    }
}

pub(crate) fn run_state_key_init(format: Format) -> ExitCode {
    let resolver = KeyResolver::new(SystemKeySources::default());
    match resolver.initialize() {
        Ok(result) => match render_state_key_init(format, &result) {
            Ok(output) => write_stdout(&output),
            Err(_) => ExitCode::from(1),
        },
        Err(error) => {
            if format == Format::Text {
                let _ = writeln!(io::stderr(), "{error}");
            } else if let Ok(output) =
                Envelope::failure(ErrorCode::Internal, error.to_string()).render(format)
            {
                let _ = io::stdout().write_all(output.as_bytes());
            }
            ExitCode::from(1)
        }
    }
}

pub(crate) fn render_state_key_init(
    format: Format,
    result: &KeyInitResult,
) -> Result<String, serde_json::Error> {
    match format {
        Format::Json => {
            let mut output = serde_json::to_string(&StateSuccess {
                success: true,
                data: result,
            })?;
            output.push('\n');
            Ok(output)
        }
        Format::Text if !result.instruction.is_empty() => Ok(format!(
            "No secure key store is available. Configure the generated key with:\n{}\n",
            result.instruction
        )),
        Format::Text => Ok(format!(
            "State encryption key {} via {}.\n",
            result.action, result.key_source
        )),
        Format::Yaml => Ok(format!(
            "success: true\ndata:\n    action: {}\n    configured: {}\n    keysource: {}\n    instruction: {}\nwarnings: []\nerror: null\n",
            result.action,
            result.configured,
            result.key_source,
            serde_json::to_string(&result.instruction)?
        )),
    }
}

pub(crate) fn run_daemon(
    session: String,
    mode: String,
    engine: String,
    ssrf: Option<bool>,
    allow_private: Option<bool>,
    format: Format,
) -> ExitCode {
    let context = match LoadContext::from_process(FlagOverrides::default()) {
        Ok(context) => context,
        Err(error) => {
            let _ = writeln!(io::stderr(), "daemon configuration: {error}");
            return ExitCode::from(1);
        }
    };
    let config = match load(&context) {
        Ok(result) => result.config,
        Err(error) => {
            if is_transport_retired(&error.to_string()) {
                return write_transport_retired(format, &error.to_string());
            }
            let _ = writeln!(io::stderr(), "daemon configuration: {error}");
            return ExitCode::from(1);
        }
    };
    let engine = if engine.is_empty() {
        if config.engine.is_empty() {
            "chrome".to_owned()
        } else {
            config.engine.clone()
        }
    } else {
        engine
    };
    let mode = if mode.is_empty() {
        config.mode.clone()
    } else {
        mode
    };
    // The static transport does not accept a browser engine. Keep the
    // browser default only for browser mode; an inherited engine must not
    // poison an explicit transport selection.
    let engine = if mode != "browser" {
        "static".to_owned()
    } else {
        engine
    };
    let selection_engine = (mode == "browser").then_some(engine.as_str());
    if let Err(error) = symbrowse_core::config::resolve_selection(Some(&mode), selection_engine) {
        if error.code == symbrowse_core::config::TRANSPORT_RETIRED {
            return write_transport_retired(format, &error.message);
        }
        let _ = writeln!(
            io::stderr(),
            "invalid transport selection: {}",
            error.message
        );
        return ExitCode::from(1);
    }
    if let Err(error) = validate_engine(&engine) {
        let _ = writeln!(io::stderr(), "{error}");
        return ExitCode::from(1);
    }
    let idle_timeout = if config.idle_timeout == 0 {
        None
    } else {
        Some(Duration::from_secs(config.idle_timeout as u64))
    };
    let mut spec = SessionSpec::from_config(&config, session.clone());
    spec.mode = mode.clone();
    spec.engine = engine.clone();
    spec.ssrf_enabled = ssrf.unwrap_or(config.ssrf_enabled);
    spec.allow_private = allow_private.unwrap_or(config.allow_private);
    spec.socket_path = default_socket_path(&session);
    let policy = PolicyStatus {
        allowed_domains: config.allowed_domains.clone(),
        ssrf_enabled: spec.ssrf_enabled,
        fetch_ssrf_enabled: spec.ssrf_enabled,
        allow_private: spec.allow_private,
    };
    let options = ServerOptions {
        socket_path: spec.socket_path.clone(),
        session: session.clone(),
        engine: engine.clone(),
        mode: mode.clone(),
        policy,
        idle_timeout,
        operation_timeout: Duration::from_secs(config.operation_timeout as u64),
        read_timeout: Duration::from_secs(config.read_timeout as u64),
        startup_provider_owner: match std::env::current_exe() {
            Ok(executable) => Some(executable),
            Err(error) => {
                return write_batch_error(format, ErrorCode::Internal, &error.to_string(), 1);
            }
        },
        session_spec: Some(spec),
        ..ServerOptions::default()
    };
    match Server::new(options).and_then(|server| server.listen_and_serve()) {
        Ok(()) | Err(symbrowse_daemon::ServerError::AlreadyRunning) => ExitCode::SUCCESS,
        Err(error) => write_batch_error(format, ErrorCode::Internal, &error.to_string(), 1),
    }
}


pub(crate) fn run_config_show(format: Format, flags: FlagOverrides) -> ExitCode {
    match render_config_show(format, flags) {
        Ok(output) => write_stdout(&output),
        Err(error) => write_config_error(format, &error),
    }
}

pub(crate) fn render_config_show(format: Format, flags: FlagOverrides) -> Result<String, String> {
    let context = match LoadContext::from_process(flags) {
        Ok(context) => context,
        Err(error) => return Err(error.to_string()),
    };
    let result = match load(&context) {
        Ok(result) => result,
        Err(error) => return Err(error.to_string()),
    };
    if format == Format::Text {
        return Ok(render_show_text(&result));
    }
    if format == Format::Json {
        let payload = ConfigSuccess {
            success: true,
            data: ConfigData {
                fields: show_fields(&result),
            },
        };
        return match serde_json::to_string(&payload) {
            Ok(mut output) => {
                output.push('\n');
                Ok(output)
            }
            Err(error) => Err(error.to_string()),
        };
    }
    Ok(render_show_yaml(&result))
}

/// True when a configuration or selection failure reports the retired
/// `compat` transport.
pub(crate) fn is_transport_retired(message: &str) -> bool {
    message.contains(symbrowse_core::config::TRANSPORT_RETIRED)
}

/// Render the retired-transport failure through the existing config error
/// contract: `config` error code, exit 9, and the stable `transport_retired`
/// identifier in `details.code`.
pub(crate) fn write_transport_retired(format: Format, message: &str) -> ExitCode {
    if format == Format::Text {
        let _ = writeln!(io::stderr(), "{message}");
    } else {
        let mut envelope = Envelope::failure(ErrorCode::Config, message);
        if let Some(error) = envelope.error.as_mut() {
            error.hint = "set mode to static or browser".to_owned();
            error.details = Some(serde_json::json!({
                "code": symbrowse_core::config::TRANSPORT_RETIRED,
            }));
        }
        if let Ok(output) = envelope.render(format) {
            let _ = io::stdout().write_all(output.as_bytes());
        }
    }
    ExitCode::from(ErrorCode::Config.exit_code())
}

pub(crate) fn write_config_error(format: Format, message: &str) -> ExitCode {
    if is_transport_retired(message) {
        return write_transport_retired(format, message);
    }
    if format == Format::Text {
        let _ = writeln!(io::stderr(), "{message}");
    } else {
        let envelope_message = message.split_once(':').map_or(message, |(outer, _)| outer);
        let envelope = Envelope::failure(ErrorCode::Config, envelope_message);
        if let Ok(output) = envelope.render(format) {
            let _ = io::stdout().write_all(output.as_bytes());
        }
    }
    ExitCode::from(9)
}


pub(crate) fn write_stdout(value: &str) -> ExitCode {
    if io::stdout().write_all(value.as_bytes()).is_err() {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}


pub(crate) fn write_batch_error(format: Format, code: ErrorCode, message: &str, exit_code: u8) -> ExitCode {
    if format == Format::Text {
        let _ = writeln!(io::stderr(), "{message}");
    } else if let Ok(output) = Envelope::failure(code, message).render(format) {
        let _ = io::stdout().write_all(output.as_bytes());
    }
    ExitCode::from(exit_code)
}
