#![deny(unsafe_code)]

mod browser_profiles;
mod completion;
mod help_catalog;
mod upgrade;

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

const VERSION: &str = match option_env!("SYMBROWSE_VERSION") {
    Some(version) => version,
    None => "dev",
};

#[derive(Debug, Eq, PartialEq)]
enum Action {
    Help(String),
    Completion {
        shell: String,
        no_descriptions: bool,
    },
    CompletionRequest {
        args: Vec<String>,
        no_descriptions: bool,
    },
    RootVersion,
    Version {
        structured: bool,
    },
    UpgradeCheck {
        format: Format,
    },
    ConfigShow {
        format: Format,
        flags: FlagOverrides,
    },
    Batch {
        format: Format,
        commands: Vec<String>,
        bail: bool,
        dry_run: bool,
    },
    StateKeyInit {
        format: Format,
    },
    DaemonRun {
        session: String,
        mode: String,
        engine: String,
        ssrf: Option<bool>,
        allow_private: Option<bool>,
    },
    DaemonLifecycle {
        session: String,
        command: String,
        format: Format,
    },
    StateOperation {
        session: String,
        command: String,
        name: Option<String>,
        older_than: Option<i64>,
        format: Format,
    },
    Mcp {
        session: String,
        profiles: String,
        allow_private: bool,
        engine: Option<String>,
    },
    McpListProfiles,
    ProfileList {
        arguments: Vec<String>,
    },
    ToolList {
        profiles: String,
        format: Format,
    },
    Dispatch {
        session: String,
        command: String,
        args: serde_json::Value,
        format: Format,
    },
    FlowList {
        format: Format,
    },
    FlowValidate {
        path: PathBuf,
        format: Format,
    },
    FlowRun {
        session: String,
        path: PathBuf,
        inputs: BTreeMap<String, String>,
        dry_run: bool,
        format: Format,
    },
}

#[derive(Debug, Eq, PartialEq)]
struct ParseError {
    message: String,
    exit_code: u8,
}

#[derive(Serialize)]
struct ConfigSuccess<T> {
    success: bool,
    data: T,
}

#[derive(Serialize)]
struct ConfigData {
    fields: std::collections::BTreeMap<String, symbrowse_core::config::Field>,
}

#[derive(Serialize)]
struct BatchSuccess<'a> {
    success: bool,
    data: &'a batch::Report,
}

#[derive(Serialize)]
struct StateSuccess<'a> {
    success: bool,
    data: &'a KeyInitResult,
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse(&args) {
        Ok(Action::Help(text)) => write_stdout(&text),
        Ok(Action::Completion {
            shell,
            no_descriptions,
        }) => {
            let script = if no_descriptions {
                completion::script_without_descriptions(&shell)
            } else {
                completion::script(&shell).map(str::to_owned)
            };
            script.map_or_else(|| ExitCode::from(2), |script| write_stdout(&script))
        }
        Ok(Action::CompletionRequest {
            args,
            no_descriptions,
        }) => {
            let output = if no_descriptions {
                completion::complete_no_descriptions(&args)
            } else {
                completion::complete(&args)
            };
            write_stdout(&output)
        }
        Ok(Action::RootVersion) => write_stdout(&render_root_version(VERSION)),
        Ok(Action::Version { structured: true }) => match render_version_json(VERSION) {
            Ok(output) => write_stdout(&output),
            Err(_) => ExitCode::from(1),
        },
        Ok(Action::Version { structured: false }) => write_stdout(&render_version_text(VERSION)),
        Ok(Action::UpgradeCheck { format }) => upgrade::run(format, VERSION),
        Ok(Action::ConfigShow { format, flags }) => run_config_show(format, flags),
        Ok(Action::Batch {
            format,
            commands,
            bail,
            dry_run,
        }) => run_batch(format, commands, bail, dry_run),
        Ok(Action::StateKeyInit { format }) => run_state_key_init(format),
        Ok(Action::DaemonRun {
            session,
            mode,
            engine,
            ssrf,
            allow_private,
        }) => run_daemon(session, mode, engine, ssrf, allow_private),
        Ok(Action::DaemonLifecycle {
            session,
            command,
            format,
        }) => run_daemon_lifecycle(session, command, format),
        Ok(Action::StateOperation {
            session,
            command,
            name,
            older_than,
            format,
        }) => run_state_operation(session, command, name, older_than, format),
        Ok(Action::Mcp {
            session,
            profiles,
            allow_private,
            engine,
        }) => run_mcp(session, profiles, allow_private, engine),
        Ok(Action::McpListProfiles) => write_stdout(MCP_PROFILE_LIST),
        Ok(Action::ProfileList { arguments }) => browser_profiles::run(&arguments),
        Ok(Action::ToolList { profiles, format }) => run_tool_list(profiles, format),
        Ok(Action::Dispatch {
            session,
            command,
            args,
            format,
        }) => run_dispatch(session, command, args, format),
        Ok(Action::FlowList { format }) => run_flow_list(format),
        Ok(Action::FlowValidate { path, format }) => run_flow_validate(path, format),
        Ok(Action::FlowRun {
            session,
            path,
            inputs,
            dry_run,
            format,
        }) => run_flow(session, path, inputs, dry_run, format),
        Err(error) => {
            let _ = writeln!(io::stderr(), "{}", error.message);
            ExitCode::from(error.exit_code)
        }
    }
}

const MCP_PROFILE_LIST: &str = "core     14 tools  Page interaction and reading: open, snapshot, click, fill, type, press, wait, read, get, find. The default profile.\n           tools: open, snapshot, click, fill, type, press, wait, read, get, find, fetch_url, fetch_batch, cache_get, wayback_snapshots\nnav       3 tools  History navigation: back, forward, reload.\n           tools: back, forward, reload\nstate     0 tools  Sessions, cookies, storage, state save/load, auth login. Tools land with the state milestone (v0.4.0).\nnetwork   0 tools  Routing, mocking, request inspection, HAR, headers, offline. Tools land with the network milestone (v1.0.0).\ndebug     0 tools  Console, errors, eval, a11y, diff, doctor. Tools land with the reach milestones (v1.0.0).\nflows     0 tools  Flow list/run/record. Tools land with the flows milestone (v0.6.0).\n";

fn run_tool_list(profiles: String, format: Format) -> ExitCode {
    let selected = match registry::validate_profile_selection(&profiles) {
        Ok(selected) => selected,
        Err(error) => return render_dispatch_error(format, daemon_codes::MALFORMED_REQUEST, error),
    };
    let tools = registry::specs()
        .iter()
        .filter(|spec| selected.contains(&spec.profile))
        .map(|spec| serde_json::json!({"name": spec.name, "command": spec.command, "profile": spec.profile}))
        .collect::<Vec<_>>();
    match Envelope::ok(serde_json::Value::Array(tools), Vec::new()).render(format) {
        Ok(output) => write_stdout(&output),
        Err(error) => {
            render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error.to_string())
        }
    }
}

fn run_dispatch(
    session: String,
    command: String,
    args: serde_json::Value,
    format: Format,
) -> ExitCode {
    let frame = Frame {
        cmd: command,
        args: Some(args),
        session: session.clone(),
        ..Frame::default()
    };
    let direct = if matches!(frame.cmd.as_str(), "fetch.url" | "fetch.batch") {
        LoadContext::from_process(FlagOverrides::default())
            .ok()
            .and_then(|context| load(&context).ok())
            .filter(|result| result.config.engine == "static")
            .map(|result| {
                dispatch_once(
                    SessionSpec::from_config(&result.config, &session),
                    frame.clone(),
                )
            })
    } else {
        None
    };
    let response = match if let Some(response) = direct {
        Ok(response)
    } else {
        Client::new(ClientOptions {
            socket_path: default_socket_path(&session),
            session,
            ..ClientOptions::default()
        })
        .request(frame)
    } {
        Ok(response) => response,
        Err(error) => {
            return render_dispatch_error(
                format,
                daemon_codes::DAEMON_UNAVAILABLE,
                error.to_string(),
            );
        }
    };
    if response.success {
        let envelope = Envelope::ok(
            response.data.unwrap_or(serde_json::Value::Null),
            response
                .warnings
                .into_iter()
                .map(|warning| symbrowse_core::output::Warning {
                    kind: warning.kind,
                    severity: warning.severity,
                    message: warning.message,
                    r#ref: warning.r#ref,
                    excerpt: warning.excerpt,
                })
                .collect(),
        );
        match envelope.render(format) {
            Ok(output) => write_stdout(&output),
            Err(error) => {
                render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error.to_string())
            }
        }
    } else {
        let error = response.error.unwrap_or_default();
        render_dispatch_error(format, &error.code, error.message)
    }
}

fn render_dispatch_error(format: Format, code: &str, message: String) -> ExitCode {
    let mapped = match code {
        daemon_codes::MALFORMED_REQUEST => ErrorCode::MalformedRequest,
        daemon_codes::UNKNOWN_COMMAND => ErrorCode::UnknownCommand,
        daemon_codes::OPERATION_TIMEOUT => ErrorCode::OperationTimeout,
        daemon_codes::PEER_DENIED => ErrorCode::PeerDenied,
        daemon_codes::DAEMON_UNAVAILABLE => ErrorCode::DaemonUnavailable,
        daemon_codes::INVALID_SESSION => ErrorCode::InvalidSession,
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

fn run_flow_list(format: Format) -> ExitCode {
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

fn run_flow_validate(path: PathBuf, format: Format) -> ExitCode {
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

fn run_flow(
    session: String,
    path: PathBuf,
    inputs: BTreeMap<String, String>,
    dry_run: bool,
    format: Format,
) -> ExitCode {
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            return render_dispatch_error(
                format,
                "not_found",
                format!("read flow {}: {error}", path.display()),
            );
        }
    };
    let client = Client::new(ClientOptions {
        socket_path: default_socket_path(&session),
        session: session.clone(),
        ..ClientOptions::default()
    });
    let response = match client.request(Frame {
        cmd: "flow.run".into(),
        args: Some(serde_json::json!({"yaml": source, "inputs": inputs, "dry_run": dry_run})),
        session,
        ..Frame::default()
    }) {
        Ok(response) => response,
        Err(error) => {
            return render_dispatch_error(
                format,
                daemon_codes::DAEMON_UNAVAILABLE,
                error.to_string(),
            );
        }
    };
    if response.success {
        match Envelope::ok(response.data.unwrap_or_default(), Vec::new()).render(format) {
            Ok(output) => write_stdout(&output),
            Err(error) => {
                render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error.to_string())
            }
        }
    } else {
        let error = response.error.unwrap_or_default();
        render_dispatch_error(format, &error.code, error.message)
    }
}

fn run_mcp(
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

fn validate_engine(value: &str) -> Result<(), String> {
    if matches!(value, "chrome" | "static" | "safari-attach" | "safari-bidi") {
        Ok(())
    } else {
        Err(format!(
            "invalid engine {value:?}: use one of chrome, static, safari-attach, safari-bidi"
        ))
    }
}

fn run_state_key_init(format: Format) -> ExitCode {
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

fn render_state_key_init(
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

fn run_daemon(
    session: String,
    mode: String,
    engine: String,
    ssrf: Option<bool>,
    allow_private: Option<bool>,
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
    // Static and compat transports do not accept a browser engine. Keep the
    // browser default only for browser mode; an inherited engine must not
    // poison an explicit transport selection.
    let engine = if mode != "browser" {
        "static".to_owned()
    } else {
        engine
    };
    let selection_engine = (mode == "browser").then_some(engine.as_str());
    if let Err(error) = symbrowse_core::config::resolve_selection(Some(&mode), selection_engine) {
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
        session_spec: Some(spec),
        ..ServerOptions::default()
    };
    match Server::new(options).and_then(|server| server.listen_and_serve()) {
        Ok(()) | Err(symbrowse_daemon::ServerError::AlreadyRunning) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr(), "daemon: {error}");
            ExitCode::from(1)
        }
    }
}

fn run_daemon_lifecycle(session: String, command: String, format: Format) -> ExitCode {
    let client = Client::new(ClientOptions {
        socket_path: default_socket_path(&session),
        session: session.clone(),
        autostart: false,
        ..ClientOptions::default()
    });
    let response = match client.request_without_autostart(Frame {
        cmd: command,
        session,
        ..Frame::default()
    }) {
        Ok(response) => response,
        Err(error) => {
            if format == Format::Text {
                let _ = writeln!(io::stderr(), "{error}");
            } else if let Ok(output) = serde_json::to_string(&symbrowse_daemon::error_response(
                daemon_codes::DAEMON_UNAVAILABLE,
                error.to_string(),
            )) {
                let _ = writeln!(io::stdout(), "{output}");
            }
            return ExitCode::from(1);
        }
    };
    if format == Format::Text {
        if response.success {
            let _ = writeln!(
                io::stdout(),
                "{}",
                response.data.as_ref().map_or_else(
                    || "ok".to_owned(),
                    |data| serde_json::to_string_pretty(data).unwrap_or_else(|_| "ok".to_owned())
                )
            );
            ExitCode::SUCCESS
        } else {
            let _ = writeln!(
                io::stderr(),
                "{}",
                response
                    .error
                    .map_or_else(|| "daemon request failed".to_owned(), |error| error.message)
            );
            ExitCode::from(1)
        }
    } else {
        serde_json::to_string(&response)
            .map(|output| write_stdout(&(output + "\n")))
            .unwrap_or_else(|_| ExitCode::from(1))
    }
}

fn run_state_operation(
    session: String,
    command: String,
    name: Option<String>,
    older_than: Option<i64>,
    format: Format,
) -> ExitCode {
    let args = match (name, older_than) {
        (Some(name), _) => Some(serde_json::json!({"name": name})),
        (None, Some(days)) => Some(serde_json::json!({"older_than_days": days})),
        (None, None) => None,
    };
    let client = Client::new(ClientOptions {
        socket_path: default_socket_path(&session),
        session: session.clone(),
        ..ClientOptions::default()
    });
    let response = match client.request(Frame {
        cmd: command.clone(),
        args,
        session,
        ..Frame::default()
    }) {
        Ok(response) => response,
        Err(error) => {
            let _ = writeln!(io::stderr(), "{error}");
            return ExitCode::from(1);
        }
    };
    if !response.success {
        let _ = writeln!(
            io::stderr(),
            "{}",
            response
                .error
                .map_or_else(|| "state request failed".to_owned(), |error| error.message)
        );
        return ExitCode::from(1);
    }
    if format != Format::Text {
        return serde_json::to_string(&response)
            .map(|output| write_stdout(&(output + "\n")))
            .unwrap_or_else(|_| ExitCode::from(1));
    }
    let data = response.data.unwrap_or(serde_json::Value::Null);
    let output = match command.as_str() {
        "state.save" => format!(
            "saved state {:?}\n",
            data.get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
        ),
        "state.load" => format!(
            "loaded state {:?}\n",
            data.get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
        ),
        "state.clear" => format!(
            "cleared state {:?}\n",
            data.get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
        ),
        "state.list" => data
            .get("states")
            .and_then(serde_json::Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(|s| format!("{s}\n"))
                    .collect()
            })
            .unwrap_or_default(),
        "state.clean" => format!(
            "removed {} expired state(s)\n",
            data.get("removed")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len)
        ),
        _ => serde_json::to_string_pretty(&data).unwrap_or_default() + "\n",
    };
    write_stdout(&output)
}

fn run_batch(format: Format, mut commands: Vec<String>, bail: bool, dry_run: bool) -> ExitCode {
    if commands.is_empty() {
        let mut input = String::new();
        if let Err(error) = io::stdin().read_to_string(&mut input) {
            return write_batch_error(
                format,
                ErrorCode::Internal,
                &format!("read batch commands from stdin: {error}"),
                1,
            );
        }
        if input.trim().is_empty() {
            return write_batch_error(
                format,
                ErrorCode::Internal,
                "stdin must contain a JSON array of command strings: unexpected end of JSON input",
                1,
            );
        }
        commands = match decode_batch_commands(&input) {
            Ok(commands) => commands,
            Err(error) => {
                return write_batch_error(
                    format,
                    ErrorCode::Internal,
                    &format!("stdin must contain a JSON array of command strings: {error}"),
                    1,
                );
            }
        };
    }
    if commands.is_empty() {
        return write_batch_error(
            format,
            ErrorCode::InvalidArgs,
            "batch requires at least one command",
            2,
        );
    }

    let report = batch::run(&commands, dry_run, bail, execute_batch_item);
    if format == Format::Json {
        return match serde_json::to_string(&BatchSuccess {
            success: true,
            data: &report,
        }) {
            Ok(mut output) => {
                output.push('\n');
                write_stdout(&output)
            }
            Err(_) => ExitCode::from(1),
        };
    }
    if format == Format::Text {
        return match serde_json::to_string_pretty(&report) {
            Ok(mut output) => {
                output.push('\n');
                write_stdout(&output)
            }
            Err(_) => ExitCode::from(1),
        };
    }
    write_stdout(&batch::render_yaml(&report))
}

fn decode_batch_commands(input: &str) -> Result<Vec<String>, String> {
    let value: serde_json::Value = serde_json::from_str(input).map_err(|error| {
        if error.is_eof() {
            return "unexpected end of JSON input".to_owned();
        }
        let trimmed = input.trim_start();
        if trimmed.starts_with('n') && trimmed != "null" {
            let expected = "null";
            if let Some((index, actual)) = trimmed
                .chars()
                .enumerate()
                .find(|(index, character)| expected.chars().nth(*index) != Some(*character))
            {
                let wanted = expected.chars().nth(index).unwrap_or(' ');
                return format!(
                    "invalid character {actual:?} in literal null (expecting {wanted:?})"
                );
            }
        }
        error.to_string()
    })?;
    let serde_json::Value::Array(items) = value else {
        if value.is_null() {
            return Ok(Vec::new());
        }
        return Err(format!(
            "json: cannot unmarshal {} into Go value of type []string",
            json_type_name(&value)
        ));
    };
    items
        .into_iter()
        .map(|item| match item {
            serde_json::Value::String(command) => Ok(command),
            serde_json::Value::Null => Ok(String::new()),
            other => Err(format!(
                "json: cannot unmarshal {} into Go value of type string",
                json_type_name(&other)
            )),
        })
        .collect()
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn execute_batch_item(argv: &[String]) -> ItemOutput {
    let args: Vec<OsString> = argv.iter().map(OsString::from).collect();
    match parse(&args) {
        Ok(Action::Help(stdout)) => ItemOutput {
            stdout,
            error: None,
        },
        Ok(Action::RootVersion) => ItemOutput {
            stdout: render_root_version(VERSION),
            error: None,
        },
        Ok(Action::Version { structured: true }) => match render_version_json(VERSION) {
            Ok(stdout) => ItemOutput {
                stdout,
                error: None,
            },
            Err(error) => ItemOutput {
                stdout: String::new(),
                error: Some(error.to_string()),
            },
        },
        Ok(Action::Version { structured: false }) => ItemOutput {
            stdout: render_version_text(VERSION),
            error: None,
        },
        Ok(Action::McpListProfiles) => ItemOutput {
            stdout: MCP_PROFILE_LIST.to_owned(),
            error: None,
        },
        Ok(Action::ConfigShow { format, flags }) => match render_config_show(format, flags) {
            Ok(stdout) => ItemOutput {
                stdout,
                error: None,
            },
            Err(error) => ItemOutput {
                stdout: String::new(),
                error: Some(error),
            },
        },
        Ok(Action::StateKeyInit { format }) => {
            let resolver = KeyResolver::new(SystemKeySources::default());
            match resolver.initialize() {
                Ok(result) => match render_state_key_init(format, &result) {
                    Ok(stdout) => ItemOutput {
                        stdout,
                        error: None,
                    },
                    Err(error) => ItemOutput {
                        stdout: String::new(),
                        error: Some(error.to_string()),
                    },
                },
                Err(error) => ItemOutput {
                    stdout: String::new(),
                    error: Some(error.to_string()),
                },
            }
        }
        Ok(_) => ItemOutput {
            stdout: String::new(),
            error: Some(format!("batch item {:?} is not available yet", argv[0])),
        },
        Err(error) => ItemOutput {
            stdout: String::new(),
            error: Some(error.message),
        },
    }
}

fn write_batch_error(format: Format, code: ErrorCode, message: &str, exit_code: u8) -> ExitCode {
    if format == Format::Text {
        let _ = writeln!(io::stderr(), "{message}");
    } else if let Ok(output) = Envelope::failure(code, message).render(format) {
        let _ = io::stdout().write_all(output.as_bytes());
    }
    ExitCode::from(exit_code)
}

fn run_config_show(format: Format, flags: FlagOverrides) -> ExitCode {
    match render_config_show(format, flags) {
        Ok(output) => write_stdout(&output),
        Err(error) => write_config_error(format, &error),
    }
}

fn render_config_show(format: Format, flags: FlagOverrides) -> Result<String, String> {
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

fn write_config_error(format: Format, message: &str) -> ExitCode {
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

fn implemented_help_command(command: &str) -> bool {
    matches!(
        command,
        "batch"
            | "back"
            | "click"
            | "config"
            | "daemon"
            | "fill"
            | "find"
            | "flow"
            | "forward"
            | "get"
            | "goto"
            | "is"
            | "mcp"
            | "open"
            | "press"
            | "profiles"
            | "read"
            | "reload"
            | "snapshot"
            | "state"
            | "type"
            | "upgrade"
            | "version"
            | "wait"
            | "tools"
            | "fetch"
            | "workflow"
    )
}

fn root_help() -> String {
    "symbrowse is the standalone command-line entrypoint for Symaira Browse.\n\nUsage:\n  symbrowse [command]\n\nCore Commands:\n  batch     Run multiple commands in one process and report per-item status\n  click     Click an element matching a selector or @ref\n  fill      Fill an input element, replacing its content\n  find      Find an element semantically and optionally act on it\n  get       Inspect page and element values\n  goto      Navigate to a URL (alias for open)\n  is        Check page and element state\n  open      Open a URL in the browser and wait for load\n  press     Press a keyboard key on an element\n  read      Render the page as markdown (or JSON) in the symfetch output schema\n  snapshot  Render the accessibility tree\n  type      Type text into an element, appending to its content\n  wait      Wait for a browser condition\n\nNavigation Commands:\n  back      Navigate back in page history\n  forward   Navigate forward in page history\n  reload    Reload the current page\n\nState Commands:\n  profiles  List discovered Chrome profiles available for reuse\n  state     Save, restore and manage named browser session states\n\nDebug Commands:\n  config    Inspect symbrowse configuration\n  daemon    Run or inspect the symbrowse daemon\n  mcp       Start the MCP stdio server (JSON-RPC 2.0 over stdin/stdout)\n  upgrade   Check for and apply symbrowse updates\n  version   Print the symbrowse version\n\nFlows Commands:\n  flow      Validate, run and record declarative browser flows\n\nAdditional Commands:\n  completion  Generate the autocompletion script for the specified shell\n  help        Help about any command\n\nFlags:\n  -h, --help            help for symbrowse\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n  -v, --version         version for symbrowse\n\nUse \"symbrowse [command] --help\" for more information about a command.\n"
        .to_owned()
}

fn command_help(command: &str, suffix: &[&str]) -> Option<String> {
    let first = suffix.first().copied();
    let target = match command {
        "workflow" => "flow",
        other => other,
    };
    let global = "Global Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n";
    let session_global = "Global Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --session string   session name (default \"default\")\n";
    let plain = |description: &str, usage: &str, flags: &str, globals: &str| {
        format!("{description}\n\nUsage:\n  {usage}\n\nFlags:\n{flags}\n{globals}")
    };
    match (target, first) {
        ("version", None) => Some(plain(
            "Print the symbrowse version",
            "symbrowse version [flags]",
            "  -h, --help   help for version\n",
            global,
        )),
        ("upgrade", None) => Some(plain(
            "upgrade checks GitHub for a newer release (cached for 24h), verifies the asset checksum (and cosign signature when available), and atomically replaces the running binary with backup and rollback. Homebrew installations are not replaced — the command prints the brew upgrade hint instead.",
            "symbrowse upgrade [flags]",
            "      --check   only check for updates, do not apply\n  -h, --help    help for upgrade\n",
            global,
        )),
        ("profiles", None) => Some(plain(
            "List discovered Chrome profiles available for reuse",
            "symbrowse profiles [flags]",
            "  -h, --help   help for profiles\n",
            global,
        )),
        ("batch", None) => Some(plain(
            "batch runs each quoted command string as a symbrowse invocation in the same process, which avoids one daemon autostart and process startup per command. Without positional arguments a JSON array of command strings is read from stdin. --bail stops at the first failure; --dry-run returns the execution plan with risk classes without executing anything.",
            "symbrowse batch <cmd> [cmd...] [flags]",
            "      --bail      stop at the first failed command\n      --dry-run   return the execution plan without executing\n  -h, --help      help for batch\n",
            global,
        )),
        ("config", None) => Some(
            "Inspect symbrowse configuration\n\nUsage:\n  symbrowse config [command]\n\nAvailable Commands:\n  show        Show the effective configuration and its source\n\nFlags:\n  -h, --help   help for config\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n\nUse \"symbrowse config [command] --help\" for more information about a command.\n".to_owned(),
        ),
        ("config", Some("show")) => Some(plain(
            "Show the effective configuration and its source",
            "symbrowse config show [flags]",
            "      --cache-dir string         override the cache directory\n      --config-dir string        override the config directory\n      --executable-path string   override the browser executable path\n  -h, --help                     help for show\n      --log-format string        override the configured log format\n      --log-level string         override the configured log level\n      --state-dir string         override the state directory\n",
            global,
        )),
        ("state", None) => Some(
            "Save, restore and manage named browser session states\n\nUsage:\n  symbrowse state [command]\n\nAvailable Commands:\n  clean       Remove expired states (or states older than --older-than days)\n  clear       Delete one named state\n  key         Provision the state-encryption key\n  list        List named states\n  load        Restore cookies and web storage from a named state\n  save        Capture cookies and web storage into a named state\n  show        Show state metadata (origins, counts, age) without values\n\nFlags:\n  -h, --help             help for state\n      --session string   session name (default \"default\")\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n\nUse \"symbrowse state [command] --help\" for more information about a command.\n".to_owned(),
        ),
        ("state", Some("key")) if suffix.get(1).is_none() => Some(
            "Provision the state-encryption key\n\nUsage:\n  symbrowse state key [command]\n\nAvailable Commands:\n  init        Generate and provision a state-encryption key without rotating an existing key\n\nFlags:\n  -h, --help   help for key\n\nGlobal Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --session string   session name (default \"default\")\n\nUse \"symbrowse state key [command] --help\" for more information about a command.\n".to_owned(),
        ),
        ("state", Some("key")) if suffix.get(1) == Some(&"init") => Some(plain(
            "Generate and provision a state-encryption key without rotating an existing key",
            "symbrowse state key init [flags]",
            "  -h, --help   help for init\n",
            session_global,
        )),
        ("state", Some(subcommand @ ("save" | "load" | "show" | "clear"))) => {
            let description = match subcommand {
                "save" => "Capture cookies and web storage into a named state",
                "load" => "Restore cookies and web storage from a named state",
                "show" => "Show state metadata (origins, counts, age) without values",
                "clear" => "Delete one named state",
                _ => unreachable!(),
            };
            Some(plain(
                description,
                &format!("symbrowse state {subcommand} <name> [flags]"),
                &format!("  -h, --help   help for {subcommand}\n"),
                session_global,
            ))
        }
        ("state", Some(subcommand @ ("list" | "clean"))) => {
            let description = if subcommand == "list" {
                "List named states"
            } else {
                "Remove expired states (or states older than --older-than days)"
            };
            let flags = if subcommand == "clean" {
                "  -h, --help                help for clean\n      --older-than string   remove states saved more than this many days ago\n"
            } else {
                "  -h, --help   help for list\n"
            };
            Some(plain(
                description,
                &format!("symbrowse state {subcommand} [flags]"),
                flags,
                session_global,
            ))
        }
        ("flow", Some("list")) => Some(plain(
            "List discovered flows with their origin",
            "symbrowse flow list [flags]",
            "  -h, --help   help for list\n",
            global,
        )),
        ("flow", Some("validate")) => Some(plain(
            "Validate a flow document with line-accurate errors",
            "symbrowse flow validate <datei> [flags]",
            "  -h, --help   help for validate\n",
            global,
        )),
        ("flow", Some("run")) => Some(plain(
            "Execute a flow step by step (assertions are hard abort conditions)",
            "symbrowse flow run <name> [flags]",
            "      --dry-run             print the execution plan with risk classes without executing\n  -h, --help                help for run\n      --input stringArray   flow input as k=v (repeatable)\n      --session string      daemon session name (default \"default\")\n",
            global,
        )),
        ("mcp", None) => Some(
            "mcp runs the Model Context Protocol stdio server. Tools proxy to the local symbrowse daemon; every tool accepts an optional session argument. No byte is written to stdout except JSON-RPC frames (zero stdout pollution); all logging goes to stderr.\n\nTool profiles select the registered tools (--tools core|nav|state|network|debug|flows|all, comma-separated combinations allowed; default core).\n\nSecurity defaults in MCP mode: the daemon is started with the SSRF guard enabled, so private and loopback targets are denied. Pass --allow-private to permit them explicitly. The domain allowlist stays configurable through the daemon flags and config.toml.\n\nThe browser engine is selected with --engine, or persistently through the engine key in config.toml (the flag wins). The selected engine is passed to the daemon this server starts.\n\nUsage:\n  symbrowse mcp [flags]\n\nFlags:\n      --allow-private    allow private and loopback targets (SSRF opt-out; MCP mode denies them by default)\n      --engine string    engine implementation: chrome (default), static (JS-free HTML reader), safari-attach (live Safari session via Apple Events), or safari-bidi (isolated Safari via safaridriver --bidi) (default \"chrome\")\n  -h, --help             help for mcp\n      --list-profiles    describe every tool profile and its tool count, then exit\n      --session string   default session for tool calls without a session argument (default \"default\")\n      --tools string     tool profiles to register: core|nav|state|network|debug|flows|all (comma-separated combinations allowed) (default \"core\")\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n".to_owned(),
        ),
        ("tools", Some("list")) => Some(plain(
            "List registered Browse tools for one or more profiles",
            "symbrowse tools list [flags]",
            "  -h, --help              help for list\n      --profiles string   comma-separated tool profiles (default \"core\")\n      --tools string      alias for --profiles\n",
            global,
        )),
        ("tools", None) => Some(
            "List registered Browse tools for one or more profiles\n\nUsage:\n  symbrowse tools [command]\n\nAvailable Commands:\n  list        List registered Browse tools for one or more profiles\n\nFlags:\n  -h, --help   help for tools\n\nUse \"symbrowse tools [command] --help\" for more information about a command.\n".to_owned(),
        ),
        ("daemon", Some("status" | "stop")) => Some(format!(
            "Usage:\n  symbrowse daemon {} [flags]\n\nUse --help with an implemented command for its usage.\n",
            first.unwrap()
        )),
        ("daemon", None | Some("run")) => Some("Run or inspect the symbrowse daemon\n\nUsage:\n  symbrowse daemon [flags]\n".to_owned()),
        ("flow", None) => Some(
            "flow manages declarative, versioned browser automation scripts. Flows are YAML documents with semantic finders, hard domain constraints and op://…-only secret references.\n\nUsage:\n  symbrowse flow [command]\n\nAvailable Commands:\n  list        List discovered flows with their origin\n  run         Execute a flow step by step (assertions are hard abort conditions)\n  validate    Validate a flow document with line-accurate errors\n\nFlags:\n  -h, --help   help for flow\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n\nUse \"symbrowse flow [command] --help\" for more information about a command.\n".to_owned(),
        ),
        ("config", Some(_)) | ("state", Some(_)) | ("flow", Some(_)) | ("tools", Some(_)) => None,
        ("open" | "goto" | "fetch", None) => {
            Some("Usage:\n  symbrowse <open|goto|fetch> <url> [flags]\n".to_owned())
        }
        (
            "back" | "click" | "fill" | "find" | "forward" | "get" | "is" | "press" | "read"
            | "reload" | "snapshot" | "type" | "wait",
            None,
        ) => Some("Usage:\n  symbrowse <command> [arguments] [flags]\n".to_owned()),
        _ => None,
    }
}

fn help_for_path(path: &[&str]) -> String {
    let Some((command, suffix)) = path.split_first() else {
        return "Help provides help for any command in the application.\nSimply type symbrowse help [path to command] for full details.\n\nUsage:\n  symbrowse help [command] [flags]\n\nFlags:\n  -h, --help   help for help\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n".to_owned();
    };
    let command = if *command == "workflow" {
        "flow"
    } else {
        command
    };
    if implemented_help_command(command) {
        if let Some(text) = help_catalog::help(command, suffix) {
            return text.to_owned();
        }
        if let Some(text) = command_help(command, suffix) {
            return text;
        }
    }
    root_help()
}

fn parse_completion(values: &[String]) -> Result<Action, ParseError> {
    if values
        .get(1)
        .is_some_and(|value| matches!(value.as_str(), "-h" | "--help"))
    {
        return Ok(Action::Help(completion_help().to_owned()));
    }
    let Some(shell) = values.get(1).map(String::as_str) else {
        return Err(ParseError {
            message: "completion requires one shell: bash, zsh, fish or powershell".into(),
            exit_code: 2,
        });
    };
    if completion::script(shell).is_none() {
        return Err(ParseError {
            message: format!("unsupported shell {shell:?}; choose bash, zsh, fish or powershell"),
            exit_code: 2,
        });
    }
    if values
        .iter()
        .skip(2)
        .any(|value| matches!(value.as_str(), "-h" | "--help"))
    {
        return Ok(Action::Help(completion_shell_help(shell)));
    }
    let mut no_descriptions = false;
    for value in values.iter().skip(2) {
        match value.as_str() {
            "--no-descriptions" => no_descriptions = true,
            value if value.starts_with("--no-descriptions=") => {
                no_descriptions = parse_bool("--no-descriptions", &value[18..])?;
            }
            value if value.starts_with('-') => {
                return Err(ParseError {
                    message: format!("unknown flag: {value}"),
                    exit_code: 2,
                });
            }
            value => {
                return Err(ParseError {
                    message: format!("unknown argument {value:?}"),
                    exit_code: 2,
                });
            }
        }
    }
    Ok(Action::Completion {
        shell: shell.to_owned(),
        no_descriptions,
    })
}

fn completion_shell_help(shell: &str) -> String {
    let (description, long) = match shell {
        "bash" => (
            "Generate the autocompletion script for the bash shell",
            "This script depends on the 'bash-completion' package.\nIf it is not installed already, you can install it via your OS's package manager.\n\nTo load completions in your current shell session:\n\n\tsource <(symbrowse completion bash)\n\nTo load completions for every new session, execute once:\n\n#### Linux:\n\n\tsymbrowse completion bash > /etc/bash_completion.d/symbrowse\n\n#### macOS:\n\n\tsymbrowse completion bash > $(brew --prefix)/etc/bash_completion.d/symbrowse\n\nYou will need to start a new shell for this setup to take effect.",
        ),
        "zsh" => (
            "Generate the autocompletion script for the zsh shell",
            "If shell completion is not already enabled in your environment you will need\nto enable it.  You can execute the following once:\n\n\techo \"autoload -U compinit; compinit\" >> ~/.zshrc\n\nTo load completions in your current shell session:\n\n\tsource <(symbrowse completion zsh)\n\nTo load completions for every new session, execute once:\n\n#### Linux:\n\n\tsymbrowse completion zsh > \"${fpath[1]}/_symbrowse\"\n\n#### macOS:\n\n\tsymbrowse completion zsh > $(brew --prefix)/share/zsh/site-functions/_symbrowse\n\nYou will need to start a new shell for this setup to take effect.",
        ),
        "fish" => (
            "Generate the autocompletion script for the fish shell",
            "To load completions in your current shell session:\n\n\tsymbrowse completion fish | source\n\nTo load completions for every new session, execute once:\n\n\tsymbrowse completion fish > ~/.config/fish/completions/symbrowse.fish\n\nYou will need to start a new shell for this setup to take effect.",
        ),
        "powershell" => (
            "Generate the autocompletion script for powershell",
            "To load completions in your current shell session:\n\n\tsymbrowse completion powershell | Out-String | Invoke-Expression\n\nTo load completions for every new session, add the output of the above command\nto your powershell profile.",
        ),
        _ => return completion_help().to_owned(),
    };
    let usage_suffix = if shell == "bash" { "" } else { " [flags]" };
    format!(
        "{description}.\n\n{long}\n\nUsage:\n  symbrowse completion {shell}{usage_suffix}\n\nFlags:\n  -h, --help              help for {shell}\n      --no-descriptions   disable completion descriptions\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n"
    )
}

fn completion_help() -> &'static str {
    "Generate the autocompletion script for symbrowse for the specified shell.\nSee each sub-command's help for details on how to use the generated script.\n\nUsage:\n  symbrowse completion [command]\n\nAvailable Commands:\n  bash        Generate the autocompletion script for bash\n  fish        Generate the autocompletion script for fish\n  powershell  Generate the autocompletion script for powershell\n  zsh         Generate the autocompletion script for zsh\n\nFlags:\n  -h, --help   help for completion\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n\nUse \"symbrowse completion [command] --help\" for more information about a command.\n"
}

fn parse(args: &[OsString]) -> Result<Action, ParseError> {
    let values: Vec<String> = args
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    if values.first().is_some_and(|value| value == "completion") {
        return parse_completion(&values);
    }
    if values
        .first()
        .is_some_and(|value| matches!(value.as_str(), "__complete" | "__completeNoDesc"))
    {
        return Ok(Action::CompletionRequest {
            args: values[1..].to_vec(),
            no_descriptions: values[0] == "__completeNoDesc",
        });
    }
    if root_version_requested(&values)? {
        return Ok(Action::RootVersion);
    }
    if let Some(help_index) = values
        .iter()
        .take_while(|value| value.as_str() != "--")
        .position(|value| matches!(value.as_str(), "-h" | "--help"))
    {
        let Some(command_index) = top_command_index(&values[..help_index]) else {
            return Ok(Action::Help(root_help()));
        };
        let command = values[command_index].as_str();
        let suffix = values[command_index + 1..help_index]
            .iter()
            .filter(|value| !value.starts_with('-'))
            .map(String::as_str)
            .collect::<Vec<_>>();
        if command == "help" {
            return Ok(Action::Help(help_for_path(&suffix)));
        }
        if implemented_help_command(command) {
            if let Some(text) = help_catalog::help(command, &suffix) {
                return Ok(Action::Help(text.to_owned()));
            }
            if let Some(text) = command_help(command, &suffix) {
                return Ok(Action::Help(text));
            }
        }
    }
    let Some(command_index) = top_command_index(&values) else {
        if let Some(value) = values.iter().find(|value| value.starts_with('-')) {
            return Err(ParseError {
                message: format!("unknown flag: {value}"),
                exit_code: 2,
            });
        }
        return Err(ParseError {
            message: "a command is required".to_owned(),
            exit_code: 2,
        });
    };
    match values[command_index].as_str() {
        "help" => Ok(Action::Help(help_for_path(
            &values[command_index + 1..]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        ))),
        "version" => parse_version(&values, command_index),
        "upgrade" => parse_upgrade(&values, command_index),
        "config" => parse_config(&values, command_index),
        "batch" => parse_batch(&values, command_index),
        "state" => parse_state_lifecycle(&values, command_index),
        "daemon" => parse_daemon(&values, command_index),
        "mcp" => parse_mcp(&values, command_index),
        "flow" | "workflow" => parse_flow(&values, command_index),
        "profiles" => {
            let mut arguments = values;
            arguments.remove(command_index);
            Ok(Action::ProfileList { arguments })
        }
        "tools" => parse_tools(&values, command_index),
        "fetch" | "read" | "open" | "goto" | "snapshot" | "click" | "fill" | "type" | "press"
        | "wait" | "back" | "forward" | "reload" | "get" | "is" | "find" => {
            parse_dispatch(&values, command_index)
        }
        command => Err(ParseError {
            message: format!("unknown command {command:?} for \"symbrowse\""),
            exit_code: 2,
        }),
    }
}

fn root_version_requested(values: &[String]) -> Result<bool, ParseError> {
    for value in values.iter().take_while(|value| value.as_str() != "--") {
        if matches!(value.as_str(), "-v" | "--version") {
            return Ok(true);
        }
        if let Some(raw) = value.strip_prefix("--version=") {
            return parse_bool("--version", raw);
        }
    }
    Ok(false)
}

fn top_command_index(values: &[String]) -> Option<usize> {
    let mut index = 0;
    while index < values.len() {
        let value = &values[index];
        if value == "--" {
            return (index + 1 < values.len()).then_some(index + 1);
        }
        if matches!(value.as_str(), "--json" | "-v" | "--version")
            || value.starts_with("--json=")
            || value.starts_with("--version=")
        {
            index += 1;
            continue;
        }
        if matches!(
            value.as_str(),
            "--output"
                | "--log-level"
                | "--log-format"
                | "--config-dir"
                | "--cache-dir"
                | "--state-dir"
                | "--executable-path"
        ) {
            index += 2;
            continue;
        }
        if value.starts_with("--output=")
            || value.starts_with("--log-level=")
            || value.starts_with("--log-format=")
            || value.starts_with("--config-dir=")
            || value.starts_with("--cache-dir=")
            || value.starts_with("--state-dir=")
            || value.starts_with("--executable-path=")
        {
            index += 1;
            continue;
        }
        if value.starts_with('-') {
            return None;
        }
        return Some(index);
    }
    None
}

fn parse_dispatch(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let name = values[command_index].as_str();
    let mut command = if name == "fetch" {
        "fetch.url".to_owned()
    } else {
        name.to_owned()
    };
    let mut session = "default".to_owned();
    let mut format = Format::Text;
    let mut positional = Vec::new();
    let mut args = serde_json::Map::new();
    let mut index = command_index + 1;
    while index < values.len() {
        let value = &values[index];
        match value.as_str() {
            "--json" => format = Format::Json,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            "--selector" => {
                index += 1;
                args.insert(
                    "selector".into(),
                    serde_json::Value::String(required_value(values, index, "--selector")?.into()),
                );
            }
            "--kind" => {
                index += 1;
                args.insert(
                    "kind".into(),
                    serde_json::Value::String(required_value(values, index, "--kind")?.into()),
                );
            }
            "--value" => {
                index += 1;
                args.insert(
                    "value".into(),
                    serde_json::Value::String(required_value(values, index, "--value")?.into()),
                );
            }
            "--key" => {
                index += 1;
                args.insert(
                    "key".into(),
                    serde_json::Value::String(required_value(values, index, "--key")?.into()),
                );
            }
            "--format" => {
                index += 1;
                args.insert(
                    "format".into(),
                    serde_json::Value::String(required_value(values, index, "--format")?.into()),
                );
            }
            "--action" | "--name" => {
                let key = value.trim_start_matches('-');
                index += 1;
                args.insert(
                    key.into(),
                    serde_json::Value::String(required_value(values, index, value)?.into()),
                );
            }
            "--exact" => {
                args.insert("exact".into(), serde_json::Value::Bool(true));
            }
            "--index" => {
                index += 1;
                let index_value = required_value(values, index, "--index")?
                    .parse::<u64>()
                    .map_err(|_| ParseError {
                        message: "invalid index".into(),
                        exit_code: 2,
                    })?;
                args.insert("index".into(), serde_json::Value::from(index_value));
            }
            "--attribute" => {
                index += 1;
                args.insert(
                    "attribute".into(),
                    serde_json::Value::String(required_value(values, index, "--attribute")?.into()),
                );
            }
            "--query" => {
                index += 1;
                args.insert(
                    "query".into(),
                    serde_json::Value::String(required_value(values, index, "--query")?.into()),
                );
            }
            "--ms" => {
                index += 1;
                let ms = required_value(values, index, "--ms")?
                    .parse::<u64>()
                    .map_err(|_| ParseError {
                        message: "invalid milliseconds".into(),
                        exit_code: 2,
                    })?;
                args.insert("ms".into(), serde_json::Value::from(ms));
            }
            "--depth" => {
                index += 1;
                let depth = required_value(values, index, "--depth")?
                    .parse::<u64>()
                    .map_err(|_| ParseError {
                        message: "invalid depth".into(),
                        exit_code: 2,
                    })?;
                args.insert("depth".into(), serde_json::Value::from(depth));
            }
            "--compact" | "--interactive" | "--urls" | "--diff" | "--engine-hint" => {
                let key = value.trim_start_matches('-').replace('-', "_");
                args.insert(key, serde_json::Value::Bool(true));
            }
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            value if value.starts_with("--kind=") => {
                args.insert("kind".into(), serde_json::Value::String(value[7..].into()));
            }
            value if value.starts_with("--selector=") => {
                args.insert(
                    "selector".into(),
                    serde_json::Value::String(value[11..].into()),
                );
            }
            value if value.starts_with("--value=") => {
                args.insert("value".into(), serde_json::Value::String(value[8..].into()));
            }
            value if value.starts_with("--key=") => {
                args.insert("key".into(), serde_json::Value::String(value[6..].into()));
            }
            value if value.starts_with("--query=") => {
                args.insert("query".into(), serde_json::Value::String(value[8..].into()));
            }
            value if value.starts_with('-') => {
                return Err(ParseError {
                    message: format!("unknown flag: {value}"),
                    exit_code: 2,
                });
            }
            _ => positional.push(value.clone()),
        }
        index += 1;
    }
    match name {
        "fetch" | "read" | "open" | "goto" => {
            take_positional(&mut args, &mut positional, "url");
        }
        "click" | "fill" => take_positional(&mut args, &mut positional, "selector"),
        "press" => take_positional(&mut args, &mut positional, "key"),
        "get" | "is" => take_positional(&mut args, &mut positional, "kind"),
        _ => {}
    }
    if name == "fill" {
        take_positional(&mut args, &mut positional, "value");
    }
    if name == "type" {
        take_positional(&mut args, &mut positional, "value");
    }
    if name == "find" {
        if !args.contains_key("kind") && !args.contains_key("query") {
            match positional.len() {
                0 => {}
                1 => {
                    args.insert("kind".into(), serde_json::Value::String("text".into()));
                    args.insert(
                        "query".into(),
                        serde_json::Value::String(positional.remove(0)),
                    );
                }
                _ => {
                    take_positional(&mut args, &mut positional, "kind");
                    take_positional(&mut args, &mut positional, "query");
                }
            }
        } else {
            take_positional(&mut args, &mut positional, "kind");
            take_positional(&mut args, &mut positional, "query");
        }
    }
    if name == "get" || name == "is" {
        take_positional(&mut args, &mut positional, "selector");
    }
    if name == "wait"
        && !args.contains_key("kind")
        && let Some(value) = positional.first()
    {
        args.insert("kind".into(), serde_json::Value::String("selector".into()));
        args.insert("selector".into(), serde_json::Value::String(value.clone()));
        positional.remove(0);
    }
    if name == "get" || name == "is" {
        let kind = args
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("text");
        command = format!("{name}.{kind}");
    }
    if !positional.is_empty() {
        return Err(ParseError {
            message: format!("unknown argument {:?}", positional[0]),
            exit_code: 2,
        });
    }
    let required = match name {
        "fetch" | "open" | "goto" => &["url"][..],
        "click" => &["selector"][..],
        "fill" => &["selector", "value"][..],
        "type" => &["value"][..],
        "press" => &["key"][..],
        "get" | "is" => &["kind"][..],
        "find" => &["kind", "query"][..],
        "wait" => &["kind"][..],
        _ => &[][..],
    };
    for required in required {
        if args
            .get(*required)
            .and_then(serde_json::Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(ParseError {
                message: format!("{name} requires {required}"),
                exit_code: 2,
            });
        }
    }
    if name == "fill"
        && args
            .get("value")
            .and_then(serde_json::Value::as_str)
            .is_none()
    {
        return Err(ParseError {
            message: "fill requires value".into(),
            exit_code: 2,
        });
    }
    Ok(Action::Dispatch {
        session,
        command,
        args: serde_json::Value::Object(args),
        format,
    })
}

fn take_positional(
    args: &mut serde_json::Map<String, serde_json::Value>,
    positional: &mut Vec<String>,
    key: &str,
) {
    if !args.contains_key(key)
        && let Some(value) = positional.first().cloned()
    {
        args.insert(key.to_owned(), serde_json::Value::String(value));
        positional.remove(0);
    }
}

fn parse_tools(values: &[String], index: usize) -> Result<Action, ParseError> {
    if values.get(index + 1).map(String::as_str) != Some("list") {
        return Err(ParseError {
            message: "tools requires list".into(),
            exit_code: 2,
        });
    }
    let mut profiles = "core".to_owned();
    let mut format = Format::Text;
    let mut i = index + 2;
    while i < values.len() {
        match values[i].as_str() {
            "--json" => format = Format::Json,
            "--output" => {
                i += 1;
                format = parse_format(required_value(values, i, "--output")?)?;
            }
            "--profiles" | "--tools" => {
                i += 1;
                profiles = required_value(values, i, "--profiles")?.to_owned();
            }
            value if value.starts_with("--profiles=") => profiles = value[11..].to_owned(),
            value if value.starts_with("--tools=") => profiles = value[8..].to_owned(),
            value => {
                return Err(ParseError {
                    message: format!("unknown argument {value:?}"),
                    exit_code: 2,
                });
            }
        }
        i += 1;
    }
    Ok(Action::ToolList { profiles, format })
}

fn parse_flow(values: &[String], index: usize) -> Result<Action, ParseError> {
    let (subcommand, mut i) = match values.get(index + 1) {
        None => ("list", index + 1),
        Some(value) if value.starts_with('-') => ("list", index + 1),
        Some(value) => (value.as_str(), index + 2),
    };
    let mut format = Format::Text;
    let mut path = None;
    let mut session = "default".to_owned();
    let mut dry_run = false;
    let mut inputs = BTreeMap::new();
    while i < values.len() {
        match values[i].as_str() {
            "--json" => format = Format::Json,
            "--dry-run" => dry_run = true,
            "--output" => {
                i += 1;
                format = parse_format(required_value(values, i, "--output")?)?;
            }
            "--session" => {
                i += 1;
                session = required_value(values, i, "--session")?.to_owned();
            }
            "--input" => {
                i += 1;
                let pair = required_value(values, i, "--input")?;
                let (key, value) = pair.split_once('=').ok_or_else(|| ParseError {
                    message: "--input requires key=value".into(),
                    exit_code: 2,
                })?;
                inputs.insert(key.to_owned(), value.to_owned());
            }
            value if value.starts_with("--input=") => {
                let pair = &value[8..];
                let (key, val) = pair.split_once('=').ok_or_else(|| ParseError {
                    message: "--input requires key=value".into(),
                    exit_code: 2,
                })?;
                inputs.insert(key.to_owned(), val.to_owned());
            }
            value if value.starts_with('-') => {
                return Err(ParseError {
                    message: format!("unknown flag: {value}"),
                    exit_code: 2,
                });
            }
            value if path.is_none() => path = Some(PathBuf::from(value)),
            value => {
                return Err(ParseError {
                    message: format!("unknown argument {value:?}"),
                    exit_code: 2,
                });
            }
        }
        i += 1;
    }
    match subcommand {
        "list" if path.is_none() => Ok(Action::FlowList { format }),
        "validate" => path
            .map(|path| Action::FlowValidate { path, format })
            .ok_or_else(|| ParseError {
                message: "flow validate requires a path".into(),
                exit_code: 2,
            }),
        "run" => path
            .map(|path| Action::FlowRun {
                session,
                path,
                inputs,
                dry_run,
                format,
            })
            .ok_or_else(|| ParseError {
                message: "flow run requires a path".into(),
                exit_code: 2,
            }),
        other => Err(ParseError {
            message: format!("unknown command {other:?} for flow"),
            exit_code: 2,
        }),
    }
}
fn parse_daemon(values: &[String], daemon_index: usize) -> Result<Action, ParseError> {
    let mut session = "default".to_owned();
    let mut engine = String::new();
    let mut mode = String::new();
    let mut ssrf = None;
    let mut allow_private = None;
    let mut format = Format::Text;
    let subcommand = values
        .get(daemon_index + 1)
        .map(String::as_str)
        .unwrap_or("");
    let mut index = daemon_index + 1;
    while index < values.len() {
        match values[index].as_str() {
            "status" | "stop" => {}
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            "--engine" => {
                index += 1;
                engine = required_value(values, index, "--engine")?.to_owned();
            }
            "--mode" => {
                index += 1;
                mode = required_value(values, index, "--mode")?.to_owned();
            }
            "--ssrf" => {
                if let Some(value @ ("true" | "false")) = values.get(index + 1).map(String::as_str)
                {
                    index += 1;
                    ssrf = Some(parse_bool("--ssrf", value)?);
                } else {
                    ssrf = Some(true);
                }
            }
            "--mcp-mode" => {}
            "--allow-private" => allow_private = Some(true),
            "--json" => format = Format::Json,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with("--engine=") => engine = value[9..].to_owned(),
            value if value.starts_with("--mode=") => mode = value[7..].to_owned(),
            value if value.starts_with("--ssrf=") => {
                ssrf = Some(parse_bool("--ssrf", &value[7..])?);
            }
            "--allow-private=true" => allow_private = Some(true),
            "--allow-private=false" => allow_private = Some(false),
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            "--json=true" => format = Format::Json,
            value if value.starts_with('-') => {
                return Err(ParseError {
                    message: format!("unknown flag: {value}"),
                    exit_code: 2,
                });
            }
            "daemon" => {}
            value => {
                return Err(ParseError {
                    message: format!("unknown command {value:?} for daemon"),
                    exit_code: 2,
                });
            }
        }
        index += 1;
    }
    if subcommand == "status" || subcommand == "stop" {
        return Ok(Action::DaemonLifecycle {
            session,
            command: format!("daemon.{subcommand}"),
            format,
        });
    }
    Ok(Action::DaemonRun {
        session,
        mode,
        engine,
        ssrf,
        allow_private,
    })
}

fn parse_mcp(values: &[String], mcp_index: usize) -> Result<Action, ParseError> {
    let mut session = String::from("default");
    let mut profiles = String::from("core");
    let mut list_profiles = false;
    let mut allow_private = false;
    let mut engine = None;
    let mut index = 0;
    while index < values.len() {
        if index == mcp_index {
            index += 1;
            continue;
        }
        let value = &values[index];
        match value.as_str() {
            "--list-profiles" => list_profiles = true,
            "--allow-private" => allow_private = true,
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            "--tools" => {
                index += 1;
                profiles = required_value(values, index, "--tools")?.to_owned();
            }
            "--engine" => {
                index += 1;
                engine = Some(required_value(values, index, "--engine")?.to_owned());
            }
            _ if value.starts_with("--session=") => session = value[10..].to_owned(),
            _ if value.starts_with("--tools=") => profiles = value[8..].to_owned(),
            _ if value.starts_with("--engine=") => engine = Some(value[9..].to_owned()),
            _ if value == "--json" || value.starts_with("--json=") => {}
            _ if value.starts_with('-') => {
                return Err(ParseError {
                    message: format!("unknown flag: {value}"),
                    exit_code: 2,
                });
            }
            _ => {
                return Err(ParseError {
                    message: format!("unknown command {value:?} for \"symbrowse mcp\""),
                    exit_code: 2,
                });
            }
        }
        index += 1;
    }
    if let Some(value) = engine.as_deref()
        && !matches!(value, "chrome" | "static" | "safari-attach" | "safari-bidi")
    {
        return Err(ParseError {
            message: format!(
                "invalid engine {value:?}: use one of chrome, static, safari-attach, safari-bidi"
            ),
            exit_code: 1,
        });
    }
    if list_profiles {
        Ok(Action::McpListProfiles)
    } else {
        Ok(Action::Mcp {
            session,
            profiles,
            allow_private,
            engine,
        })
    }
}

fn parse_state_lifecycle(values: &[String], state_index: usize) -> Result<Action, ParseError> {
    let subcommand = values
        .get(state_index + 1)
        .map(String::as_str)
        .ok_or_else(|| ParseError {
            message: "state requires a subcommand".into(),
            exit_code: 2,
        })?;
    if subcommand == "key" {
        return parse_state(values, state_index);
    }
    let mut session = "default".to_owned();
    let mut output = "text".to_owned();
    let mut json = false;
    let mut name = None;
    let mut older_than = None;
    let mut index = state_index + 2;
    while index < values.len() {
        match values[index].as_str() {
            "--json" => json = true,
            "--output" => {
                index += 1;
                output = required_value(values, index, "--output")?.to_owned();
            }
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            "--older-than" => {
                index += 1;
                older_than = Some(
                    required_value(values, index, "--older-than")?
                        .parse()
                        .map_err(|_| ParseError {
                            message: "invalid day count".into(),
                            exit_code: 2,
                        })?,
                );
            }
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            value if value.starts_with("--output=") => output = value[9..].to_owned(),
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with('-') => {
                return Err(ParseError {
                    message: format!("unknown flag: {value}"),
                    exit_code: 2,
                });
            }
            value if name.is_none() => name = Some(value.to_owned()),
            value => {
                return Err(ParseError {
                    message: format!("unknown argument {value:?}"),
                    exit_code: 2,
                });
            }
        }
        index += 1;
    }
    let format = if json {
        Format::Json
    } else {
        parse_format(&output)?
    };
    let command = match subcommand {
        "save" | "load" | "show" | "clear" | "list" | "clean" => format!("state.{subcommand}"),
        _ => {
            return Err(ParseError {
                message: format!("unknown command {subcommand:?} for state"),
                exit_code: 2,
            });
        }
    };
    if matches!(subcommand, "save" | "load" | "show" | "clear") && name.is_none() {
        return Err(ParseError {
            message: format!("state {subcommand} requires a name"),
            exit_code: 2,
        });
    }
    Ok(Action::StateOperation {
        session,
        command,
        name,
        older_than,
        format,
    })
}

fn parse_state(values: &[String], state_index: usize) -> Result<Action, ParseError> {
    let key_index = values
        .iter()
        .enumerate()
        .skip(state_index + 1)
        .find_map(|(index, value)| (value == "key").then_some(index))
        .ok_or_else(|| ParseError {
            message: "state requires a subcommand".to_owned(),
            exit_code: 2,
        })?;
    let init_index = values
        .iter()
        .enumerate()
        .skip(key_index + 1)
        .find_map(|(index, value)| (value == "init").then_some(index))
        .ok_or_else(|| ParseError {
            message: "state key requires a subcommand".to_owned(),
            exit_code: 2,
        })?;
    let mut json = false;
    let mut output = String::from("text");
    let mut index = 0;
    while index < values.len() {
        if matches!(index, value if value == state_index || value == key_index || value == init_index)
        {
            index += 1;
            continue;
        }
        let value = &values[index];
        match value.as_str() {
            "--json" => json = true,
            "--output" => {
                index += 1;
                output = required_value(values, index, "--output")?.to_owned();
            }
            _ if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            _ if value.starts_with("--output=") => output = value[9..].to_owned(),
            _ if value.starts_with('-') => {
                return Err(ParseError {
                    message: format!("unknown flag: {value}"),
                    exit_code: 2,
                });
            }
            _ => {
                return Err(ParseError {
                    message: format!("unknown command {value:?} for \"symbrowse state key init\""),
                    exit_code: 2,
                });
            }
        }
        index += 1;
    }
    Ok(Action::StateKeyInit {
        format: if json {
            Format::Json
        } else {
            parse_format(&output)?
        },
    })
}

fn parse_batch(values: &[String], batch_index: usize) -> Result<Action, ParseError> {
    let mut json = false;
    let mut output = String::from("text");
    let mut bail = false;
    let mut dry_run = false;
    let mut commands = Vec::new();
    let mut index = 0;
    let mut positional_only = false;
    while index < values.len() {
        if index == batch_index {
            index += 1;
            continue;
        }
        let value = &values[index];
        if positional_only {
            commands.push(value.clone());
            index += 1;
            continue;
        }
        match value.as_str() {
            "--" => positional_only = true,
            "--json" => json = true,
            "--bail" => bail = true,
            "--dry-run" => dry_run = true,
            "--output" => {
                index += 1;
                output = required_value(values, index, "--output")?.to_owned();
            }
            _ if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            _ if value.starts_with("--bail=") => bail = parse_bool("--bail", &value[7..])?,
            _ if value.starts_with("--dry-run=") => {
                dry_run = parse_bool("--dry-run", &value[10..])?;
            }
            _ if value.starts_with("--output=") => output = value[9..].to_owned(),
            _ if value.starts_with('-') => {
                return Err(ParseError {
                    message: format!("unknown flag: {value}"),
                    exit_code: 2,
                });
            }
            _ => commands.push(value.clone()),
        }
        index += 1;
    }
    let format = if json {
        Format::Json
    } else {
        parse_format(&output)?
    };
    Ok(Action::Batch {
        format,
        commands,
        bail,
        dry_run,
    })
}

fn parse_bool(name: &str, value: &str) -> Result<bool, ParseError> {
    match value {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Ok(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Ok(false),
        _ => Err(ParseError {
            message: format!(
                "invalid argument {value:?} for {name:?} flag: strconv.ParseBool: parsing {value:?}: invalid syntax"
            ),
            exit_code: 2,
        }),
    }
}

fn parse_version(values: &[String], version_index: usize) -> Result<Action, ParseError> {
    let mut json = false;
    let mut output = String::from("text");
    let mut index = 0;
    let mut after_separator = false;
    while index < values.len() {
        let value = &values[index];
        if index == version_index {
            index += 1;
            continue;
        }
        if after_separator {
            return Err(unknown_version_argument(value));
        }
        match value.as_str() {
            "--" => after_separator = true,
            "--json" => json = true,
            "--output" => {
                index += 1;
                output = required_value(values, index, "--output")?.to_owned();
            }
            _ if value.starts_with("--output=") => output = value[9..].to_owned(),
            _ => return Err(unknown_version_argument(value)),
        }
        index += 1;
    }
    let format = parse_format(&output)?;
    Ok(Action::Version {
        structured: json || format != Format::Text,
    })
}

fn parse_upgrade(values: &[String], upgrade_index: usize) -> Result<Action, ParseError> {
    let mut check = false;
    let mut json = false;
    let mut output = "text";
    let mut index = 0;
    while index < values.len() {
        if index == upgrade_index {
            index += 1;
            continue;
        }
        match values[index].as_str() {
            "--check" => check = true,
            value if value.starts_with("--check=") => check = parse_bool("--check", &value[8..])?,
            "--json" => json = true,
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            "--output" => {
                index += 1;
                output = required_value(values, index, "--output")?;
            }
            value if value.starts_with("--output=") => output = &value[9..],
            value if value.starts_with('-') => {
                return Err(ParseError {
                    message: format!("unknown flag: {value}"),
                    exit_code: 2,
                });
            }
            value => {
                return Err(ParseError {
                    message: format!("unknown command {value:?} for \"symbrowse upgrade\""),
                    exit_code: 2,
                });
            }
        }
        index += 1;
    }
    if !check {
        return Err(ParseError {
            message: "upgrade currently supports only --check; applying an update is not implemented in this Rust slice".to_owned(),
            exit_code: 2,
        });
    }
    Ok(Action::UpgradeCheck {
        format: if json {
            Format::Json
        } else {
            parse_format(output)?
        },
    })
}

fn parse_config(values: &[String], config_index: usize) -> Result<Action, ParseError> {
    let Some(show_index) = values
        .iter()
        .enumerate()
        .skip(config_index + 1)
        .find_map(|(index, value)| (value == "show").then_some(index))
    else {
        return Err(ParseError {
            message: "config requires the show subcommand".to_owned(),
            exit_code: 2,
        });
    };
    let mut json = false;
    let mut output = String::from("text");
    let mut flags = FlagOverrides::default();
    let mut index = 0;
    while index < values.len() {
        if index == config_index || index == show_index {
            index += 1;
            continue;
        }
        let value = &values[index];
        match value.as_str() {
            "--json" => json = true,
            "--output" => {
                index += 1;
                output = required_value(values, index, "--output")?.to_owned();
            }
            "--log-level" => set_flag(values, &mut index, "--log-level", &mut flags.log_level)?,
            "--log-format" => set_flag(values, &mut index, "--log-format", &mut flags.log_format)?,
            "--config-dir" => set_flag(values, &mut index, "--config-dir", &mut flags.config_dir)?,
            "--cache-dir" => set_flag(values, &mut index, "--cache-dir", &mut flags.cache_dir)?,
            "--state-dir" => set_flag(values, &mut index, "--state-dir", &mut flags.state_dir)?,
            "--executable-path" => set_flag(
                values,
                &mut index,
                "--executable-path",
                &mut flags.executable_path,
            )?,
            "--mode" => set_flag(values, &mut index, "--mode", &mut flags.mode)?,
            "--engine" => set_flag(values, &mut index, "--engine", &mut flags.engine)?,
            _ if value.starts_with("--output=") => output = value[9..].to_owned(),
            _ => {
                return Err(ParseError {
                    message: if value.starts_with('-') {
                        format!("unknown flag: {value}")
                    } else {
                        format!("unknown command {value:?} for \"symbrowse config show\"")
                    },
                    exit_code: 2,
                });
            }
        }
        index += 1;
    }
    let format = if json {
        Format::Json
    } else {
        parse_format(&output)?
    };
    Ok(Action::ConfigShow { format, flags })
}

fn set_flag(
    values: &[String],
    index: &mut usize,
    name: &str,
    target: &mut Option<String>,
) -> Result<(), ParseError> {
    *index += 1;
    *target = Some(required_value(values, *index, name)?.to_owned());
    Ok(())
}

fn required_value<'a>(
    values: &'a [String],
    index: usize,
    name: &str,
) -> Result<&'a str, ParseError> {
    values
        .get(index)
        .map(String::as_str)
        .ok_or_else(|| ParseError {
            message: format!("flag needs an argument: {name}"),
            exit_code: 2,
        })
}

fn parse_format(value: &str) -> Result<Format, ParseError> {
    match value {
        "text" => Ok(Format::Text),
        "json" => Ok(Format::Json),
        "yaml" => Ok(Format::Yaml),
        _ => Err(ParseError {
            message: format!("invalid --output format {value:?}: want text, json or yaml"),
            exit_code: 2,
        }),
    }
}

fn unknown_version_argument(value: &str) -> ParseError {
    ParseError {
        message: format!("unknown command {value:?} for \"symbrowse version\""),
        exit_code: 2,
    }
}

fn write_stdout(value: &str) -> ExitCode {
    if io::stdout().write_all(value.as_bytes()).is_err() {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::{Action, Format, KeyInitResult, ParseError, parse, render_state_key_init};
    use std::ffi::OsString;

    use sha2::{Digest, Sha256};

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_version_flag_permutations() {
        for values in [
            &["--json", "version"][..],
            &["version", "--json"][..],
            &["--output=json", "version"][..],
            &["version", "--output", "yaml"][..],
        ] {
            assert_eq!(
                parse(&args(values)),
                Ok(Action::Version { structured: true })
            );
        }
        assert_eq!(
            parse(&args(&["version"])),
            Ok(Action::Version { structured: false })
        );
    }

    #[test]
    fn parses_config_show() {
        let parsed = parse(&args(&["config", "show", "--json", "--state-dir", "state"]))
            .expect("parse config show");
        let Action::ConfigShow { format, flags } = parsed else {
            panic!("wrong action");
        };
        assert_eq!(format, Format::Json);
        assert_eq!(flags.state_dir.as_deref(), Some("state"));
    }

    #[test]
    fn parses_batch_flags_and_commands() {
        assert_eq!(
            parse(&args(&[
                "--json",
                "batch",
                "--bail",
                "--dry-run",
                "version --json",
            ])),
            Ok(Action::Batch {
                format: Format::Json,
                commands: vec!["version --json".to_owned()],
                bail: true,
                dry_run: true,
            })
        );
    }

    #[test]
    fn batch_mcp_list_profiles_matches_source_pinned_go_output() {
        const GO_ORACLE_COMMIT: &str = "f27c09780e076ab69f16c20195dd3265f6cab037";
        const SOURCE_HASHES: [&str; 2] = [
            "2b227ad48822e009f769b7cd9340b16148da3ac28788c164bb284a75f56d7d2f",
            "7dbe1c0ab408d8ad69776fd5858f6db65be041ecfa8c8108cb86f58a45dffb46",
        ];
        let go_sources = [
            include_bytes!("../../../cmd/symbrowse/mcp.go").as_slice(),
            include_bytes!("../../../internal/mcp/profiles.go").as_slice(),
        ];
        for (source, expected_hash) in go_sources.into_iter().zip(SOURCE_HASHES) {
            let actual_hash: String = Sha256::digest(source)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            assert_eq!(
                actual_hash, expected_hash,
                "Go oracle at {GO_ORACLE_COMMIT}"
            );
        }

        let expected = include_str!("../tests/fixtures/mcp-list-profiles.stdout");
        let commands = vec!["mcp --list-profiles".to_owned()];
        let report = super::batch::run(&commands, false, false, super::execute_batch_item);

        assert_eq!(report.results.len(), 1);
        assert!(report.results[0].success);
        assert_eq!(
            report.results[0].data,
            Some(serde_json::Value::String(
                expected.trim_end_matches('\n').to_owned()
            ))
        );
    }

    #[test]
    fn state_key_init_parser_and_output_match_go() {
        assert_eq!(
            parse(&args(&["--json", "state", "key", "init"])),
            Ok(Action::StateKeyInit {
                format: Format::Json
            })
        );
        let result = KeyInitResult {
            action: "already_configured".to_owned(),
            configured: true,
            key_source: "symvault".to_owned(),
            instruction: String::new(),
        };
        assert_eq!(
            render_state_key_init(Format::Text, &result).unwrap(),
            "State encryption key already_configured via symvault.\n"
        );
        assert_eq!(
            render_state_key_init(Format::Json, &result).unwrap(),
            "{\"success\":true,\"data\":{\"action\":\"already_configured\",\"configured\":true,\"key_source\":\"symvault\"}}\n"
        );
        assert_eq!(
            render_state_key_init(Format::Yaml, &result).unwrap(),
            "success: true\ndata:\n    action: already_configured\n    configured: true\n    keysource: symvault\n    instruction: \"\"\nwarnings: []\nerror: null\n"
        );
    }

    #[test]
    fn parses_mcp_daemon_autostart_flags() {
        assert_eq!(
            parse(&args(&[
                "daemon",
                "--session",
                "auto",
                "--engine",
                "static",
                "--ssrf",
                "--mcp-mode",
            ])),
            Ok(Action::DaemonRun {
                session: "auto".to_owned(),
                mode: String::new(),
                engine: "static".to_owned(),
                ssrf: Some(true),
                allow_private: None,
            })
        );
    }

    #[test]
    fn mcp_rejects_unknown_engine_like_go() {
        let error = parse(&args(&["mcp", "--engine", "netscape"]))
            .expect_err("unknown engine must fail before stdio starts");
        assert_eq!(error.exit_code, 1);
        assert_eq!(
            error.message,
            "invalid engine \"netscape\": use one of chrome, static, safari-attach, safari-bidi"
        );
    }

    #[test]
    fn preserves_version_errors() {
        assert_eq!(
            parse(&args(&["version", "extra"])),
            Err(ParseError {
                message: "unknown command \"extra\" for \"symbrowse version\"".to_owned(),
                exit_code: 2,
            })
        );
        assert_eq!(
            parse(&args(&["version", "--output", "wat"])),
            Err(ParseError {
                message: "invalid --output format \"wat\": want text, json or yaml".to_owned(),
                exit_code: 2,
            })
        );
    }

    #[test]
    fn browser_fetch_and_flow_commands_are_typed_dispatches() {
        let Action::Dispatch {
            command,
            args: payload,
            format,
            ..
        } = parse(&args(&["fetch", "https://example.com", "--json"])).expect("fetch dispatch")
        else {
            panic!("wrong fetch action")
        };
        assert_eq!(command, "fetch.url");
        assert_eq!(format, Format::Json);
        assert_eq!(payload["url"], "https://example.com");

        let Action::Dispatch {
            command,
            args: payload,
            ..
        } = parse(&args(&["get", "title"])).expect("get dispatch")
        else {
            panic!("wrong get action")
        };
        assert_eq!(command, "get.title");
        assert_eq!(payload["kind"], "title");

        let Action::Dispatch {
            command,
            args: payload,
            ..
        } = parse(&args(&["find", "text", "hello"])).expect("find dispatch")
        else {
            panic!("wrong find action")
        };
        assert_eq!(command, "find");
        assert_eq!(payload["kind"], "text");
        assert_eq!(payload["query"], "hello");

        let Action::Dispatch { args: payload, .. } =
            parse(&args(&["find", "hello"])).expect("default text find")
        else {
            panic!("wrong default find action")
        };
        assert_eq!(payload["kind"], "text");
        assert_eq!(payload["query"], "hello");

        let Action::FlowRun { dry_run, path, .. } =
            parse(&args(&["workflow", "run", "demo.yaml", "--dry-run"])).expect("flow dispatch")
        else {
            panic!("wrong flow action")
        };
        assert!(dry_run);
        assert_eq!(path, std::path::PathBuf::from("demo.yaml"));
    }

    #[test]
    fn unknown_public_commands_fail_instead_of_succeeding() {
        for command in ["fetchx", "browser", "workflow-nope"] {
            let error = parse(&args(&[command])).expect_err("unknown command must fail");
            assert_eq!(error.exit_code, 2);
            assert!(error.message.contains("unknown command"));
        }
        assert!(parse(&args(&["flow", "nope"])).is_err());
    }
}
