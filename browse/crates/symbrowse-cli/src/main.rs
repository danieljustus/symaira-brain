#![deny(unsafe_code)]

mod browser_profiles;
mod completion;
mod doctor;
mod help_catalog;
mod upgrade;

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use base64::{
    Engine as _, alphabet,
    engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use symbrowse_core::{
    batch::{self, ItemOutput},
    cache::Cache,
    config::{
        FlagOverrides, LoadContext, SelectionView, explicit_selection, load, render_show_text,
        render_show_yaml, show_fields,
    },
    error::ErrorCode,
    flows,
    journal::Entry as JournalEntry,
    key_resolver::{KeyInitResult, KeyResolver},
    key_sources::SystemKeySources,
    oob::parse_timeout,
    output::{Envelope, Format},
    trace,
};
use symbrowse_daemon::{
    Client, ClientError, ClientOptions, DaemonError, Frame, PolicyStatus, Server, ServerOptions,
    SessionSpec, codes as daemon_codes, default_socket_path, dispatch_once,
};
use symbrowse_mcp::{ServeOptions, registry, serve_stdio};
use symbrowse_protocol::{render_root_version, render_version_json, render_version_text};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const VERSION: &str = match option_env!("SYMBROWSE_VERSION") {
    Some(version) => version,
    None => "dev",
};
const DEVICE_PROFILES: &str = include_str!("../../../internal/engine/devices.json");

const GO_STANDARD_BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true),
);
static CLI_REQUEST_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize)]
struct AuthLoginEnvelope<'a> {
    success: bool,
    data: &'a serde_json::Value,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<symbrowse_core::output::Warning>,
}

#[derive(Debug, Eq, PartialEq)]
enum Action {
    Help(String),
    CompatSidecar,
    Completion {
        shell: String,
    },
    CompletionRequest {
        args: Vec<String>,
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
    Doctor {
        format: Format,
        fix: bool,
    },
    Downloads {
        session: String,
        dir: Option<String>,
        format: Format,
    },
    SetDeviceList {
        format: Format,
    },
    RuntimeEvents {
        session: String,
        command: String,
        max_tokens: Option<i64>,
        format: Format,
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
    CookieImport {
        session: String,
        path: PathBuf,
    },
    CacheGet {
        id: String,
        range: Option<String>,
        format: Format,
    },
    CacheClear {
        format: Format,
    },
    CacheList {
        format: Format,
    },
    SessionId {
        scope: String,
        prefix: String,
        format: Format,
    },
    StorageGet {
        session: String,
        kind: String,
        key: Option<String>,
        format: Format,
    },
    Eval {
        session: String,
        expression: Option<String>,
        from_stdin: bool,
        base64: bool,
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
    TraceExport {
        session: String,
        path: PathBuf,
        format: Format,
    },
    TraceReplay {
        session: String,
        path: PathBuf,
        format: Format,
    },
    DiffSnapshot {
        session: String,
        baseline: Option<PathBuf>,
        format: Format,
    },
    DiffUrl {
        session: String,
        first_url: String,
        second_url: String,
        format: Format,
    },
    Watch {
        session: String,
        take_over: bool,
        reason: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    selection: Option<SelectionView>,
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

#[derive(Serialize)]
struct SessionIdInfo {
    id: String,
    scope: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    prefix: String,
    origin_path: String,
    #[serde(skip_serializing_if = "is_false")]
    fallback: bool,
}

#[derive(Serialize)]
struct TraceFileOutput {
    schema_version: i64,
    created_at: String,
    session: String,
    steps: Vec<TraceStepOutput>,
}

#[derive(Serialize)]
struct TraceStepOutput {
    command: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    selector: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    value: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    key: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    expected_url: String,
}

fn is_false(value: &bool) -> bool {
    !value
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse(&args) {
        Ok(Action::Help(text)) => write_stdout(&text),
        Ok(Action::CompatSidecar) => run_compat_sidecar(),
        Ok(Action::Completion { shell }) => match completion::script(&shell) {
            Some(script) => write_stdout(script),
            None => {
                let _ = writeln!(io::stderr(), "unsupported shell {:?}", shell);
                ExitCode::from(2)
            }
        },
        Ok(Action::CompletionRequest { args }) => write_stdout(&completion::complete(&args)),
        Ok(Action::RootVersion) => write_stdout(&render_root_version(VERSION)),
        Ok(Action::Version { structured: true }) => match render_version_json(VERSION) {
            Ok(output) => write_stdout(&output),
            Err(_) => ExitCode::from(1),
        },
        Ok(Action::Version { structured: false }) => write_stdout(&render_version_text(VERSION)),
        Ok(Action::UpgradeCheck { format }) => upgrade::run(format, VERSION),
        Ok(Action::ConfigShow { format, flags }) => run_config_show(format, flags),
        Ok(Action::Doctor { format, fix }) => doctor::run(format, fix),
        Ok(Action::Downloads {
            session,
            dir,
            format,
        }) => run_downloads(session, dir, format),
        Ok(Action::SetDeviceList { format }) => run_set_device_list(format),
        Ok(Action::RuntimeEvents {
            session,
            command,
            max_tokens,
            format,
        }) => run_runtime_events(session, command, max_tokens, format),
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
        Ok(Action::CookieImport { session, path }) => run_cookie_import(session, path),
        Ok(Action::CacheGet { id, range, format }) => run_cache_get(id, range, format),
        Ok(Action::CacheClear { format }) => run_cache_clear(format),
        Ok(Action::CacheList { format }) => run_cache_list(format),
        Ok(Action::SessionId {
            scope,
            prefix,
            format,
        }) => run_session_id(&scope, &prefix, format),
        Ok(Action::StorageGet {
            session,
            kind,
            key,
            format,
        }) => run_storage_get(session, kind, key, format),
        Ok(Action::Eval {
            session,
            expression,
            from_stdin,
            base64,
            format,
        }) => run_eval(session, expression, from_stdin, base64, format),
        Ok(Action::FlowList { format }) => run_flow_list(format),
        Ok(Action::FlowValidate { path, format }) => run_flow_validate(path, format),
        Ok(Action::FlowRun {
            session,
            path,
            inputs,
            dry_run,
            format,
        }) => run_flow(session, path, inputs, dry_run, format),
        Ok(Action::TraceExport {
            session,
            path,
            format,
        }) => run_trace_export(session, path, format),
        Ok(Action::TraceReplay {
            session,
            path,
            format,
        }) => run_trace_replay(session, path, format),
        Ok(Action::DiffSnapshot {
            session,
            baseline,
            format,
        }) => run_diff_snapshot(session, baseline, format),
        Ok(Action::DiffUrl {
            session,
            first_url,
            second_url,
            format,
        }) => run_diff_url(session, first_url, second_url, format),
        Ok(Action::Watch {
            session,
            take_over,
            reason,
            format,
        }) => run_watch(session, take_over, reason, format),
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
    mut args: serde_json::Value,
    format: Format,
) -> ExitCode {
    if command == "network.requests" {
        return run_network_requests(session, args, format);
    }
    let cookie_reveal = if command == "cookies.list" {
        args.as_object_mut()
            .and_then(|args| args.remove("reveal"))
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let a11y_url = if command == "a11y" {
        args.as_object_mut()
            .and_then(|args| args.remove("url"))
            .and_then(|url| url.as_str().map(str::to_owned))
    } else {
        None
    };
    let frame = cli_frame(
        &command,
        &session,
        (!matches!(
            command.as_str(),
            "session.list" | "session.info" | "cookies.list"
        ))
        .then_some(args),
    );
    let is_network_offline = frame.cmd == "network.offline";
    let is_set_mutation = matches!(
        frame.cmd.as_str(),
        "set.viewport" | "set.device" | "set.geo" | "set.headers" | "set.media" | "set.user-agent"
    );
    let is_auth_login = frame.cmd == "auth.login";
    let is_oob_status = frame.cmd == "oob.status";
    let is_handoff = frame.cmd == "handoff";
    let is_network_request = frame.cmd == "network.request";
    let is_screenshot = frame.cmd == "screenshot";
    let is_storage_mutation = matches!(frame.cmd.as_str(), "storage.set" | "storage.clear");
    let is_cookie_clear = frame.cmd == "cookies.clear";
    let is_cookie_set = frame.cmd == "cookies.set";
    let is_cookie_list = frame.cmd == "cookies.list";
    let is_upload = frame.cmd == "upload";
    let is_policy_explain = frame.cmd == "policy.explain";
    let is_journal_read = matches!(frame.cmd.as_str(), "journal.tail" | "journal.show");
    let direct = if matches!(frame.cmd.as_str(), "fetch.url" | "fetch.batch") {
        LoadContext::from_process(FlagOverrides::default())
            .ok()
            .and_then(|context| load(&context).ok())
            .filter(|result| result.config.engine == "static" || result.config.mode == "static")
            .map(|result| {
                dispatch_once(
                    SessionSpec::from_config(&result.config, &session),
                    frame.clone(),
                )
            })
    } else {
        None
    };
    let handoff_read_timeout = is_handoff.then(|| {
        frame
            .args
            .as_ref()
            .and_then(|args| args.get("timeout"))
            .and_then(serde_json::Value::as_str)
            .and_then(parse_timeout)
            .unwrap_or(Duration::from_secs(300))
            + Duration::from_secs(2)
    });
    let client = Client::new(ClientOptions {
        socket_path: default_socket_path(&session),
        session: session.clone(),
        read_timeout: handoff_read_timeout.unwrap_or(Duration::ZERO),
        ..ClientOptions::default()
    });
    if let Some(url) = a11y_url {
        let open_response = match client.request(cli_frame(
            "open",
            &session,
            Some(serde_json::json!({"url": url})),
        )) {
            Ok(response) => response,
            Err(error) => {
                return render_client_error(format, error);
            }
        };
        if !open_response.success {
            let error = open_response.error.unwrap_or_default();
            return render_dispatch_error(format, &error.code, error.message);
        }
    }
    let response = match if let Some(response) = direct {
        Ok(response)
    } else {
        client.request(frame)
    } {
        Ok(response) => response,
        Err(error) => {
            return render_client_error(format, error);
        }
    };
    if response.success {
        if is_handoff && format == Format::Text {
            let data = response.data.unwrap_or_default();
            return match serde_json::to_string_pretty(&data) {
                Ok(output) => write_stdout(&format!("{output}\n")),
                Err(error) => {
                    render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error.to_string())
                }
            };
        }
        if is_oob_status && format == Format::Text {
            let data = response.data.unwrap_or_default();
            if data.get("active").and_then(serde_json::Value::as_bool) != Some(true) {
                return write_stdout("no pending oob prompt\n");
            }
            let prompt = &data["prompt"];
            return write_stdout(&format!(
                "{}\t{}\t{}\n",
                prompt["id"].as_str().unwrap_or_default(),
                prompt["kind"].as_str().unwrap_or_default(),
                prompt["reason"].as_str().unwrap_or_default(),
            ));
        }
        if is_auth_login && format == Format::Text {
            return write_stdout("credentials entered; press enter or submit the form to log in\n");
        }
        if is_auth_login {
            let data = response.data.unwrap_or(serde_json::Value::Null);
            let mut output = serde_json::Map::new();
            for key in ["status", "url", "username_set", "password_set"] {
                if let Some(value) = data.get(key) {
                    output.insert(key.to_owned(), value.clone());
                }
            }
            if let Some(hint) = data
                .get("hint")
                .filter(|value| !value.as_str().unwrap_or_default().is_empty())
            {
                output.insert("hint".to_owned(), hint.clone());
            }
            let output = serde_json::Value::Object(output);
            let warnings = response
                .warnings
                .into_iter()
                .map(|warning| symbrowse_core::output::Warning {
                    kind: warning.kind,
                    severity: warning.severity,
                    message: warning.message,
                    r#ref: warning.r#ref,
                    excerpt: warning.excerpt,
                })
                .collect();
            let rendered = if format == Format::Json {
                serde_json::to_string(&AuthLoginEnvelope {
                    success: true,
                    data: &output,
                    warnings,
                })
                .map(|mut rendered| {
                    rendered.push('\n');
                    rendered
                })
                .map_err(|error| error.to_string())
            } else {
                serde_json::to_value(output)
                    .map_err(|error| error.to_string())
                    .and_then(|data| {
                        Envelope::ok(data, warnings)
                            .render(format)
                            .map_err(|error| error.to_string())
                    })
            };
            return match rendered {
                Ok(output) => write_stdout(&output),
                Err(error) => render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error),
            };
        }
        if is_screenshot && format == Format::Text {
            let data = response.data.unwrap_or(serde_json::Value::Null);
            let path = data
                .get("path")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let width = data
                .get("width")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let height = data
                .get("height")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let bytes = data
                .get("bytes")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            return write_stdout(&format!(
                "screenshot saved to {path} ({width}x{height}, {bytes} bytes)\n"
            ));
        }
        if is_storage_mutation && format == Format::Text {
            return write_stdout("ok\n");
        }
        if is_network_offline && format == Format::Text {
            return write_stdout("ok\n");
        }
        if is_set_mutation && format == Format::Text {
            return write_stdout("ok\n");
        }
        if is_cookie_clear && format == Format::Text {
            return write_stdout("ok\n");
        }
        if is_cookie_set && format == Format::Text {
            return write_stdout("ok\n");
        }
        let response_data = response.data.unwrap_or(serde_json::Value::Null);
        if is_network_request && format == Format::Text {
            return match serde_json::to_string_pretty(&response_data) {
                Ok(mut output) => {
                    output.push('\n');
                    write_stdout(&output)
                }
                Err(error) => {
                    render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error.to_string())
                }
            };
        }
        if is_journal_read && format == Format::Text {
            return write_stdout(&render_journal_text(&response_data));
        }
        if is_policy_explain && format == Format::Text {
            let explanation = response_data
                .get("explanation")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            return write_stdout(&format!("{explanation}\n"));
        }
        if is_upload && format == Format::Text {
            let count = response_data
                .get("uploaded")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len);
            return write_stdout(&format!("uploaded {count} file(s)\n"));
        }
        if is_cookie_list && format == Format::Text {
            return match render_cookie_list_text(&response_data, &cookie_reveal) {
                Ok(output) => write_stdout(&output),
                Err(error) => render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error),
            };
        }
        if is_cookie_list && format == Format::Json {
            let warnings = response
                .warnings
                .into_iter()
                .map(|warning| symbrowse_core::output::Warning {
                    kind: warning.kind,
                    severity: warning.severity,
                    message: warning.message,
                    r#ref: warning.r#ref,
                    excerpt: warning.excerpt,
                })
                .collect();
            return match render_cookie_list_json(&response_data, &cookie_reveal, warnings) {
                Ok(output) => write_stdout(&output),
                Err(error) => render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error),
            };
        }
        if is_cookie_list && format == Format::Yaml {
            let warnings = response
                .warnings
                .into_iter()
                .map(|warning| symbrowse_core::output::Warning {
                    kind: warning.kind,
                    severity: warning.severity,
                    message: warning.message,
                    r#ref: warning.r#ref,
                    excerpt: warning.excerpt,
                })
                .collect();
            let data = mask_cookie_list(response_data, &cookie_reveal);
            return match render_cookie_list_yaml(&data, warnings) {
                Ok(output) => write_stdout(&output),
                Err(error) => render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error),
            };
        }
        let response_data = if is_cookie_list {
            mask_cookie_list(response_data, &cookie_reveal)
        } else {
            response_data
        };
        let envelope = Envelope::ok(
            response_data,
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
        render_daemon_error(format, response.error.unwrap_or_default())
    }
}

fn run_downloads(session: String, dir: Option<String>, format: Format) -> ExitCode {
    let client = Client::new(ClientOptions {
        socket_path: default_socket_path(&session),
        session: session.clone(),
        ..ClientOptions::default()
    });
    if let Some(dir) = dir.filter(|dir| !dir.is_empty()) {
        let response = match client.request(cli_frame(
            "download.setdir",
            &session,
            Some(serde_json::json!({"dir": dir})),
        )) {
            Ok(response) => response,
            Err(error) => return render_client_error(format, error),
        };
        if !response.success {
            return render_daemon_error(format, response.error.unwrap_or_default());
        }
        if let Err(error) = writeln!(io::stdout(), "download directory set to {dir}") {
            let _ = writeln!(io::stderr(), "{error}");
            return ExitCode::from(1);
        }
    }

    let response = match client.request(cli_frame("downloads.list", &session, None)) {
        Ok(response) => response,
        Err(error) => return render_client_error(format, error),
    };
    if !response.success {
        return render_daemon_error(format, response.error.unwrap_or_default());
    }
    if format != Format::Text {
        return match Envelope::ok(
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
        )
        .render(format)
        {
            Ok(output) => write_stdout(&output),
            Err(error) => {
                render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error.to_string())
            }
        };
    }
    let data = response.data.unwrap_or(serde_json::Value::Null);
    let downloads = data
        .get("downloads")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten();
    for entry in downloads {
        let state = entry
            .get("state")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let filename = entry
            .get("filename")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let url = entry
            .get("url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let sha256 = entry
            .get("sha256")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let mut line = format!("[{state}] {filename} ({url})");
        if state == "completed" && !sha256.is_empty() {
            line.push_str(&format!(" sha256:{sha256}"));
        }
        if let Err(error) = writeln!(io::stdout(), "{line}") {
            let _ = writeln!(io::stderr(), "{error}");
            return ExitCode::from(1);
        }
    }
    ExitCode::SUCCESS
}

fn run_set_device_list(format: Format) -> ExitCode {
    let names = match device_profile_names() {
        Ok(names) => names,
        Err(error) => {
            let _ = writeln!(io::stderr(), "decode device list: {error}");
            return ExitCode::from(1);
        }
    };
    if format == Format::Text {
        return write_stdout(&format!("{}\n", names.join("\n")));
    }
    match Envelope::ok(serde_json::json!({"devices": names}), Vec::new()).render(format) {
        Ok(output) => write_stdout(&output),
        Err(_) => ExitCode::from(1),
    }
}

fn device_profile_names() -> Result<Vec<String>, serde_json::Error> {
    #[derive(serde::Deserialize)]
    struct DeviceName {
        name: String,
    }
    let mut devices: Vec<DeviceName> = serde_json::from_str(DEVICE_PROFILES)?;
    devices.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(devices.into_iter().map(|device| device.name).collect())
}

fn run_runtime_events(
    session: String,
    command: String,
    max_tokens: Option<i64>,
    format: Format,
) -> ExitCode {
    let client = Client::new(ClientOptions {
        socket_path: default_socket_path(&session),
        session: session.clone(),
        ..ClientOptions::default()
    });
    let mut frame = cli_frame(&command, &session, None);
    frame.max_tokens = max_tokens;
    let response = match client.request(frame) {
        Ok(response) => response,
        Err(error) => return render_client_error(format, error),
    };
    if !response.success {
        return render_daemon_error(format, response.error.unwrap_or_default());
    }
    let data = response.data.unwrap_or(serde_json::Value::Null);
    if command.ends_with(".clear") {
        if format == Format::Text {
            return write_stdout(if command == "console.clear" {
                "console cleared\n"
            } else {
                "errors cleared\n"
            });
        }
    } else if format == Format::Text {
        let entries = data
            .get("entries")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten();
        let mut output = String::new();
        for entry in entries {
            let text = entry
                .get("text")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if command == "console.list" {
                let kind = entry
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                output.push_str(&format!("[{kind}] {text}\n"));
            } else {
                output.push_str(text);
                output.push('\n');
                if let Some(frames) = entry
                    .get("stacktrace")
                    .and_then(serde_json::Value::as_array)
                {
                    for frame in frames {
                        let rendered = frame
                            .as_str()
                            .map_or_else(|| frame.to_string(), str::to_owned);
                        output.push_str("    at ");
                        output.push_str(&rendered);
                        output.push('\n');
                    }
                }
            }
        }
        return write_stdout(&output);
    }
    let warnings = response
        .warnings
        .into_iter()
        .map(|warning| symbrowse_core::output::Warning {
            kind: warning.kind,
            severity: warning.severity,
            message: warning.message,
            r#ref: warning.r#ref,
            excerpt: warning.excerpt,
        })
        .collect();
    match Envelope::ok(data, warnings).render(format) {
        Ok(output) => write_stdout(&output),
        Err(error) => {
            render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error.to_string())
        }
    }
}

fn run_network_requests(session: String, args: serde_json::Value, format: Format) -> ExitCode {
    let filter = args
        .get("filter")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_lowercase();
    let request_type = args
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_lowercase();
    let method = args
        .get("method")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_lowercase();
    let status = args
        .get("status")
        .and_then(serde_json::Value::as_i64)
        .filter(|status| *status > 0);
    let max_tokens = args
        .get("max_tokens")
        .and_then(serde_json::Value::as_i64)
        .filter(|value| *value > 0);
    let client = Client::new(ClientOptions {
        socket_path: default_socket_path(&session),
        session: session.clone(),
        ..ClientOptions::default()
    });
    for cmd in ["network.capture", "network.requests"] {
        let mut frame = cli_frame(cmd, &session, None);
        frame.max_tokens = if cmd == "network.requests" {
            max_tokens
        } else {
            None
        };
        let response = match client.request(frame) {
            Ok(response) => response,
            Err(error) => return render_client_error(format, error),
        };
        if !response.success {
            return render_daemon_error(format, response.error.unwrap_or_default());
        }
        if cmd == "network.requests" {
            let requests = response
                .data
                .as_ref()
                .and_then(|data| data.get("requests"))
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            let filtered = requests
                .into_iter()
                .filter(|entry| {
                    let field = |name: &str| {
                        entry
                            .get(name)
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                    };
                    let status_matches = status.is_none_or(|expected| {
                        entry
                            .get("status")
                            .and_then(serde_json::Value::as_f64)
                            .is_some_and(|actual| actual as i64 == expected)
                    });
                    (filter.is_empty() || field("url").to_lowercase().contains(&filter))
                        && (request_type.is_empty() || field("type").to_lowercase() == request_type)
                        && (method.is_empty() || field("method").to_lowercase() == method)
                        && status_matches
                })
                .collect::<Vec<_>>();
            if format == Format::Text {
                let mut output = String::new();
                for entry in &filtered {
                    let field = |name: &str| {
                        entry
                            .get(name)
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                    };
                    output.push_str(&format!(
                        "{} {} {}\n",
                        field("method"),
                        field("url"),
                        field("type")
                    ));
                }
                return write_stdout(&output);
            }
            let list = if filtered.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::Value::Array(filtered.clone())
            };
            let output = match serde_json::to_string_pretty(&serde_json::json!({
                "requests": list,
                "count": filtered.len(),
            })) {
                Ok(mut output) => {
                    output.push('\n');
                    output
                }
                Err(error) => {
                    return render_dispatch_error(
                        format,
                        daemon_codes::OPERATION_FAILED,
                        error.to_string(),
                    );
                }
            };
            return write_stdout(&output);
        }
    }
    ExitCode::SUCCESS
}

fn run_cookie_import(session: String, path: PathBuf) -> ExitCode {
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) => {
            let _ = writeln!(io::stderr(), "open curl cookie jar: {error}");
            return ExitCode::from(1);
        }
    };
    let client = Client::new(ClientOptions {
        socket_path: default_socket_path(&session),
        session: session.clone(),
        ..ClientOptions::default()
    });
    let mut imported = 0usize;
    let mut skipped = 0usize;
    for line in contents.lines() {
        let Some(cookie) = parse_curl_cookie_line(line) else {
            skipped += 1;
            continue;
        };
        let response = match client.request(Frame {
            cmd: "cookies.set".into(),
            args: Some(serde_json::json!({"cookie":cookie,"url":""})),
            session: session.clone(),
            ..Frame::default()
        }) {
            Ok(response) => response,
            Err(error) => {
                return render_dispatch_error(
                    Format::Text,
                    daemon_codes::DAEMON_UNAVAILABLE,
                    error.to_string(),
                );
            }
        };
        if response.success {
            imported += 1;
        } else {
            skipped += 1;
        }
    }
    write_stdout(&format!(
        "imported {imported} cookie(s), skipped {skipped}\n"
    ))
}

fn parse_curl_cookie_line(line: &str) -> Option<serde_json::Value> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let fields = line.split_whitespace().collect::<Vec<_>>();
    if fields.len() < 7 {
        return None;
    }
    let expires = fields[4].parse::<f64>().unwrap_or(0.0);
    Some(serde_json::json!({
        "name": fields[5],
        "value": fields[6],
        "domain": fields[0],
        "path": fields[2],
        "expires": expires,
        "size": 0,
        "secure": fields[3].eq_ignore_ascii_case("TRUE"),
        "http_only": false,
        "session": false,
    }))
}

fn reveal_cookie(name: &str, reveal: &str) -> bool {
    reveal == "all" || reveal.split(',').any(|allowed| allowed.trim() == name)
}

fn masked_cookie_value(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let bytes = value.as_bytes();
    if bytes.len() <= 8 {
        return "••••".to_owned();
    }
    let mut masked = Vec::with_capacity(16);
    masked.extend_from_slice(&bytes[..4]);
    masked.extend_from_slice("••••".as_bytes());
    masked.extend_from_slice(&bytes[bytes.len() - 4..]);
    String::from_utf8_lossy(&masked).into_owned()
}

fn mask_cookie_list(mut data: serde_json::Value, reveal: &str) -> serde_json::Value {
    if let Some(cookies) = data
        .get_mut("cookies")
        .and_then(serde_json::Value::as_array_mut)
    {
        for cookie in cookies {
            let name = cookie
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if !reveal_cookie(name, reveal) {
                let replacement = cookie
                    .get("value")
                    .and_then(serde_json::Value::as_str)
                    .map(masked_cookie_value);
                if let Some(replacement) = replacement {
                    cookie["value"] = serde_json::Value::String(replacement);
                }
            }
        }
    }
    data
}

fn cli_frame(cmd: &str, session: &str, args: Option<serde_json::Value>) -> Frame {
    Frame {
        cmd: cmd.to_owned(),
        args,
        session: session.to_owned(),
        request_id: CLI_REQUEST_ID
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1)
            .to_string(),
        retrieval_surface: "cli".to_owned(),
        ..Frame::default()
    }
}

fn run_watch(session: String, take_over: bool, reason: String, format: Format) -> ExitCode {
    if take_over {
        let args = serde_json::json!({"reason": reason, "timeout": "5m"});
        if format != Format::Text {
            return run_dispatch(session, "handoff".into(), args, format);
        }
        let client = Client::new(ClientOptions {
            socket_path: default_socket_path(&session),
            session: session.clone(),
            ..ClientOptions::default()
        });
        let response = match client.request(cli_frame("handoff", &session, Some(args))) {
            Ok(response) => response,
            Err(error) => return render_client_error(format, error),
        };
        if !response.success {
            return render_daemon_error(format, response.error.unwrap_or_default());
        }
        let data = response.data.unwrap_or(serde_json::Value::Null);
        let status = data
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let prompt_id = data
            .get("prompt_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if !status.is_empty() {
            return if prompt_id.is_empty() {
                write_stdout(&format!("handoff {status}\n"))
            } else {
                write_stdout(&format!("handoff {status} (prompt {prompt_id})\n"))
            };
        }
        return match serde_json::to_string_pretty(&data) {
            Ok(output) => write_stdout(&format!("{output}\n")),
            Err(error) => {
                render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error.to_string())
            }
        };
    }
    let banner = format!("watching session {session:?} (read-only; Ctrl-C to stop)\n");
    if io::stdout().write_all(banner.as_bytes()).is_err() || io::stdout().flush().is_err() {
        return ExitCode::from(1);
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = writeln!(io::stderr(), "start watch runtime: {error}");
            return ExitCode::from(1);
        }
    };
    runtime.block_on(watch_session(session))
}

async fn watch_session(session: String) -> ExitCode {
    let client = Client::new(ClientOptions {
        socket_path: default_socket_path(&session),
        session: session.clone(),
        ..ClientOptions::default()
    });
    let mut seen = 0;
    let shutdown = wait_watch_shutdown();
    tokio::pin!(shutdown);
    loop {
        let response = client.request(cli_frame("journal.show", &session, None));
        let delay = match response {
            Ok(response) if !response.success => {
                return render_daemon_error(Format::Text, response.error.unwrap_or_default());
            }
            Ok(response) => {
                let entries = response
                    .data
                    .as_ref()
                    .and_then(|data| data.get("entries"))
                    .and_then(serde_json::Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                for entry in entries.iter().skip(seen) {
                    let field = |name| {
                        entry
                            .get(name)
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                    };
                    let line = format!(
                        "{}\t{}\tclass={}\tdecider={}\t{}\n",
                        field("timestamp"),
                        field("command"),
                        field("risk_class"),
                        field("decider"),
                        field("result")
                    );
                    if io::stdout().write_all(line.as_bytes()).is_err()
                        || io::stdout().flush().is_err()
                    {
                        return ExitCode::from(1);
                    }
                }
                seen = entries.len();
                Duration::from_millis(500)
            }
            Err(_) => Duration::from_secs(1),
        };
        tokio::select! {
            _ = &mut shutdown => return ExitCode::SUCCESS,
            _ = tokio::time::sleep(delay) => {}
        }
    }
}

#[cfg(unix)]
async fn wait_watch_shutdown() {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("register SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = terminate.recv() => {},
    }
}

#[cfg(not(unix))]
async fn wait_watch_shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}

fn run_trace_export(session: String, path: PathBuf, format: Format) -> ExitCode {
    let client = Client::new(ClientOptions {
        socket_path: default_socket_path(&session),
        session: session.clone(),
        ..ClientOptions::default()
    });
    let response = match client.request(Frame {
        cmd: "journal.show".into(),
        args: Some(serde_json::json!({"session": session})),
        session: session.clone(),
        ..Frame::default()
    }) {
        Ok(response) => response,
        Err(error) => return render_client_error(format, error),
    };
    if !response.success {
        return render_daemon_error(format, response.error.unwrap_or_default());
    }
    let entries = response
        .data
        .as_ref()
        .and_then(|data| data.get("entries"))
        .cloned()
        .and_then(|entries| serde_json::from_value::<Vec<JournalEntry>>(entries).ok())
        .unwrap_or_default();
    let created_at = match now_rfc3339() {
        Ok(created_at) => created_at,
        Err(error) => {
            return render_dispatch_error(
                format,
                daemon_codes::OPERATION_FAILED,
                format!("format trace timestamp: {error}"),
            );
        }
    };
    let file = trace::export(&entries, session.clone(), created_at);
    if file.steps.is_empty() {
        return render_dispatch_error(
            format,
            daemon_codes::OPERATION_FAILED,
            format!("no replayable steps in the journal of session {session:?}"),
        );
    }
    let output = TraceFileOutput {
        schema_version: file.schema_version,
        created_at: file.created_at,
        session: file.session,
        steps: file
            .steps
            .into_iter()
            .map(|step| TraceStepOutput {
                command: step.command,
                selector: step.selector,
                value: step.value,
                key: step.key,
                url: step.url,
                expected_url: step.expected_url,
            })
            .collect(),
    };
    let bytes = match serde_json::to_string_pretty(&output) {
        Ok(json) => go_json_html_escape(json).into_bytes(),
        Err(error) => {
            return render_dispatch_error(
                format,
                daemon_codes::OPERATION_FAILED,
                format!("marshal trace: {error}"),
            );
        }
    };
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let write_result = options
        .open(&path)
        .and_then(|mut output| output.write_all(&bytes));
    if let Err(error) = write_result {
        return render_dispatch_error(format, "operation_failed", format!("write trace: {error}"));
    }
    let step_count = output.steps.len();
    let data = serde_json::json!({"steps": step_count, "file": path.display().to_string()});
    if format == Format::Text {
        write_stdout(&format!(
            "exported {step_count} step(s) to {}\n",
            path.display()
        ))
    } else {
        match Envelope::ok(data, Vec::new()).render(format) {
            Ok(output) => write_stdout(&output),
            Err(error) => {
                render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error.to_string())
            }
        }
    }
}

fn run_trace_replay(session: String, path: PathBuf, format: Format) -> ExitCode {
    let raw = match fs::read(&path) {
        Ok(raw) => raw,
        Err(error) => {
            return render_trace_input_error(format, format!("read trace: {error}"));
        }
    };
    let file = match serde_json::from_slice::<trace::File>(&raw) {
        Ok(file) => file,
        Err(error) => {
            return render_trace_input_error(format, format!("parse trace: {error}"));
        }
    };
    if file.schema_version != trace::SCHEMA_VERSION {
        return render_trace_input_error(
            format,
            format!("unsupported trace schema version {}", file.schema_version),
        );
    }
    if file.steps.is_empty() {
        let client = Client::new(ClientOptions {
            socket_path: default_socket_path(&session),
            session: session.clone(),
            ..ClientOptions::default()
        });
        let response = match client.request(cli_frame(
            "trace.replay",
            &session,
            Some(serde_json::json!({"steps": []})),
        )) {
            Ok(response) => response,
            Err(error) => return render_client_error(format, error),
        };
        if !response.success {
            return render_trace_daemon_error(format, response.error.unwrap_or_default());
        }
        return render_trace_input_error(format, "trace contains no replayable steps".to_owned());
    }
    let client = Client::new(ClientOptions {
        socket_path: default_socket_path(&session),
        session: session.clone(),
        ..ClientOptions::default()
    });
    let mut result = trace::ReplayResult {
        total: file.steps.len(),
        matched: 0,
        deviated: 0,
        failed: 0,
        outcomes: Vec::with_capacity(file.steps.len()),
    };
    for (index, step) in file.steps.iter().enumerate() {
        let mut outcome = trace::ReplayOutcome {
            index,
            command: step.command.clone(),
            matched: false,
            expected_url: String::new(),
            actual_url: String::new(),
            error: String::new(),
        };
        if step.command == "auth.login" {
            outcome.error =
                "credential step requires symvault re-resolution; replay it with auth login".into();
        } else if let Some((command, args)) = replay_step_frame(step) {
            let response = match client.request(cli_frame(&command, &session, Some(args))) {
                Ok(response) => response,
                Err(error) => return render_client_error(format, error),
            };
            if response.success {
                if matches!(step.command.as_str(), "open" | "goto") {
                    outcome.expected_url = step.expected_url.clone();
                    outcome.actual_url = response
                        .data
                        .as_ref()
                        .and_then(|data| data.get("url"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    outcome.matched = normalize_trace_url(&outcome.actual_url)
                        == normalize_trace_url(&outcome.expected_url);
                } else {
                    outcome.matched = true;
                }
            } else {
                outcome.error = response.error.unwrap_or_default().message;
            }
        } else {
            outcome.error = format!("step command {:?} is not replayable", step.command);
        }
        if !outcome.error.is_empty() {
            result.failed += 1;
        } else if outcome.matched {
            result.matched += 1;
        } else {
            result.deviated += 1;
        }
        result.outcomes.push(outcome);
    }
    let mut data = match serde_json::to_value(result) {
        Ok(data) => data,
        Err(error) => {
            return render_dispatch_error(
                format,
                daemon_codes::OPERATION_FAILED,
                format!("serialize trace replay: {error}"),
            );
        }
    };
    if let Some(outcomes) = data
        .get_mut("outcomes")
        .and_then(serde_json::Value::as_array_mut)
    {
        for outcome in outcomes {
            if let Some(outcome) = outcome.as_object_mut() {
                outcome.retain(|key, value| {
                    !matches!(key.as_str(), "expected_url" | "actual_url" | "error")
                        || value.as_str().is_some_and(|value| !value.is_empty())
                });
            }
        }
    }
    if format == Format::Text {
        return match serde_json::to_string_pretty(&data) {
            Ok(mut output) => {
                output.push('\n');
                write_stdout(&output)
            }
            Err(error) => render_dispatch_error(
                format,
                daemon_codes::OPERATION_FAILED,
                format!("format trace replay: {error}"),
            ),
        };
    }
    match Envelope::ok(data, Vec::new()).render(format) {
        Ok(output) => write_stdout(&output),
        Err(error) => {
            render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error.to_string())
        }
    }
}

fn render_trace_input_error(format: Format, message: String) -> ExitCode {
    let envelope = Envelope::failure(ErrorCode::Internal, message);
    if format == Format::Text {
        if let Some(error) = envelope.error.as_ref() {
            let _ = writeln!(io::stderr(), "{}", error.message);
        }
    } else {
        render_trace_error_envelope(envelope, format);
    }
    // The Go CLI maps unclassified trace file errors to ExitSoftware (1).
    ExitCode::from(1)
}

fn render_trace_daemon_error(format: Format, error: DaemonError) -> ExitCode {
    let mapped = error_code(&error.code);
    let message = format!("{message}: {message}", message = error.message);
    if format == Format::Text {
        let _ = writeln!(io::stderr(), "{message}");
    } else {
        let mut envelope = Envelope::failure(mapped, message);
        if let Some(payload) = envelope.error.as_mut() {
            payload.hint = error.hint;
            payload.details = error.details;
            payload.retryable = error.retryable;
            payload.requires_user_confirmation = error.requires_user_confirmation;
            payload.resume_hint = error.resume_hint;
        }
        render_trace_error_envelope(envelope, format);
    }
    ExitCode::from(mapped.exit_code())
}

fn render_trace_error_envelope(envelope: Envelope, format: Format) {
    if format == Format::Yaml {
        let message = envelope
            .error
            .as_ref()
            .map(|error| error.message.as_str())
            .unwrap_or_default()
            .to_owned();
        if let Ok(output) = envelope.render(format) {
            let output = if message.contains(": ") {
                output
                    .lines()
                    .map(|line| {
                        if line.starts_with("    message: ") {
                            format!("    message: {}\n", yaml_single_quote(&message))
                        } else {
                            format!("{line}\n")
                        }
                    })
                    .collect::<String>()
            } else {
                output
            };
            let output = if output.contains("    hint:") {
                output
            } else {
                output.replace("    details:", "    hint: \"\"\n    details:")
            };
            let _ = write_stdout(&output);
        } else {
            let _ = writeln!(io::stderr(), "dispatch failed");
        }
    } else {
        render_envelope_error(envelope, format);
    }
}

fn yaml_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn replay_step_frame(step: &trace::Step) -> Option<(String, serde_json::Value)> {
    let command = step.command.as_str();
    let action_args = |selector: &str| serde_json::json!({"action": command, "selector": selector});
    let args = match command {
        "open" | "goto" => serde_json::json!({"url": step.url}),
        "click" | "dblclick" | "hover" | "focus" | "check" | "uncheck" | "scrollintoview" => {
            action_args(&step.selector)
        }
        "scroll" => serde_json::json!({
            "action": command,
            "selector": step.selector,
            "amount": 1,
        }),
        "fill" | "type" | "select" => serde_json::json!({
            "action": command,
            "selector": step.selector,
            "value": step.value,
        }),
        "press" => serde_json::json!({
            "action": command,
            "selector": "body",
            "key": step.key,
        }),
        _ => return None,
    };
    Some((command.to_owned(), args))
}

fn normalize_trace_url(url: &str) -> String {
    url.split('#')
        .next()
        .unwrap_or(url)
        .trim_end_matches('/')
        .to_owned()
}

fn run_diff_snapshot(session: String, baseline: Option<PathBuf>, format: Format) -> ExitCode {
    let has_baseline = baseline.is_some();
    let before = if let Some(path) = baseline.as_ref() {
        let raw = match fs::read(path) {
            Ok(raw) => raw,
            Err(error) => {
                let _ = writeln!(io::stderr(), "read baseline {:?}: {error}", path);
                return ExitCode::from(1);
            }
        };
        let stored = match serde_json::from_slice::<serde_json::Value>(&raw) {
            Ok(stored) => stored,
            Err(error) => {
                let _ = writeln!(io::stderr(), "decode baseline {:?}: {error}", path);
                return ExitCode::from(1);
            }
        };
        match stored.get("tree") {
            None => Some(String::new()),
            Some(tree) => match tree.as_str() {
                Some(tree) => Some(tree.to_owned()),
                None => {
                    let _ = writeln!(
                        io::stderr(),
                        "decode baseline {:?}: tree must be a string",
                        path
                    );
                    return ExitCode::from(1);
                }
            },
        }
    } else {
        None
    };
    let client = Client::new(ClientOptions {
        socket_path: default_socket_path(&session),
        session: session.clone(),
        ..ClientOptions::default()
    });
    let response = match client.request(Frame {
        cmd: "snapshot".into(),
        args: Some(if baseline.is_none() {
            serde_json::json!({"diff": true})
        } else {
            serde_json::json!({})
        }),
        session,
        ..Frame::default()
    }) {
        Ok(response) => response,
        Err(error) => return render_client_error(format, error),
    };
    if !response.success {
        return render_daemon_error(format, response.error.unwrap_or_default());
    }
    let data = if let Some(before) = before.as_deref() {
        let after = response
            .data
            .as_ref()
            .and_then(|data| data.get("tree"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        snapshot_tree_diff(before, after)
    } else {
        response.data.unwrap_or(serde_json::Value::Null)
    };
    let warnings = if has_baseline {
        Vec::new()
    } else {
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
            .collect()
    };
    match Envelope::ok(data, warnings).render(format) {
        Ok(output) => write_stdout(&output),
        Err(error) => {
            render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error.to_string())
        }
    }
}

fn snapshot_tree_diff(before: &str, after: &str) -> serde_json::Value {
    let before_lines = snapshot_lines(before);
    let after_lines = snapshot_lines(after);
    let before_set: std::collections::BTreeSet<_> = before_lines.iter().collect();
    let after_set: std::collections::BTreeSet<_> = after_lines.iter().collect();
    let stable = before_lines
        .iter()
        .filter(|line| after_set.contains(line))
        .count();
    let removed: Vec<_> = before_lines
        .iter()
        .filter(|line| !after_set.contains(line))
        .map(|line| (*line).to_owned())
        .collect();
    let added: Vec<_> = after_lines
        .iter()
        .filter(|line| !before_set.contains(line))
        .map(|line| (*line).to_owned())
        .collect();
    let diff = removed
        .iter()
        .map(|line| format!("- {line}"))
        .chain(added.iter().map(|line| format!("+ {line}")))
        .collect::<Vec<_>>();
    serde_json::json!({"added": added, "diff": diff, "removed": removed, "stable": stable})
}

fn snapshot_lines(value: &str) -> Vec<&str> {
    value.split('\n').filter(|line| !line.is_empty()).collect()
}

fn run_diff_url(
    session: String,
    first_url: String,
    second_url: String,
    format: Format,
) -> ExitCode {
    let client = Client::new(ClientOptions {
        socket_path: default_socket_path(&session),
        session: session.clone(),
        ..ClientOptions::default()
    });
    let mut contents = Vec::with_capacity(2);
    for url in [first_url, second_url] {
        let response = match client.request(Frame {
            cmd: "read".into(),
            args: Some(serde_json::json!({"url": url})),
            session: session.clone(),
            ..Frame::default()
        }) {
            Ok(response) => response,
            Err(error) => return render_client_error(format, error),
        };
        if !response.success {
            return render_daemon_error(format, response.error.unwrap_or_default());
        }
        let title = response
            .data
            .as_ref()
            .and_then(|data| data.get("title"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let html = response
            .data
            .as_ref()
            .and_then(|data| data.get("html"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        contents.push(format!("{title}\n{html}"));
    }
    let data = snapshot_tree_diff(&contents[0], &contents[1]);
    match Envelope::ok(data, Vec::new()).render(format) {
        Ok(output) => write_stdout(&output),
        Err(error) => {
            render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error.to_string())
        }
    }
}

fn go_json_html_escape(json: String) -> String {
    json.replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn now_rfc3339() -> Result<String, time::error::Format> {
    OffsetDateTime::now_utc().format(&Rfc3339)
}

fn render_cookie_list_text(data: &serde_json::Value, reveal: &str) -> Result<String, String> {
    let cookies = data
        .get("cookies")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "cookie response has no cookies array".to_owned())?;
    let mut output = String::new();
    for cookie in cookies {
        let get_string = |key: &str| {
            cookie
                .get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
        };
        let name = get_string("name");
        let raw_value = get_string("value");
        let value = if reveal_cookie(name, reveal) {
            raw_value.to_owned()
        } else {
            masked_cookie_value(raw_value)
        };
        let mut flags = Vec::new();
        if cookie
            .get("secure")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            flags.push("secure");
        }
        if cookie
            .get("http_only")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            flags.push("httpOnly");
        }
        if cookie
            .get("session")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            flags.push("session");
        }
        let flags = if flags.is_empty() {
            "-".to_owned()
        } else {
            flags.join(",")
        };
        output.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            name,
            value,
            get_string("domain"),
            get_string("path"),
            flags
        ));
    }
    Ok(output)
}

struct CookieCliNumber(f64);

impl Serialize for CookieCliNumber {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if self.0.fract() == 0.0 && self.0 >= i64::MIN as f64 && self.0 <= i64::MAX as f64 {
            serializer.serialize_i64(self.0 as i64)
        } else {
            serializer.serialize_f64(self.0)
        }
    }
}

#[derive(Serialize)]
struct CookieCliItem<'a> {
    name: &'a str,
    value: String,
    domain: &'a str,
    path: &'a str,
    expires: CookieCliNumber,
    size: i64,
    http_only: bool,
    secure: bool,
    session: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    same_site: Option<&'a str>,
}

#[derive(Serialize)]
struct CookieCliData<'a> {
    origin: &'a str,
    cookies: Vec<CookieCliItem<'a>>,
}

#[derive(Serialize)]
struct CookieCliEnvelope<'a> {
    success: bool,
    data: CookieCliData<'a>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<symbrowse_core::output::Warning>,
}

fn render_cookie_list_json(
    data: &serde_json::Value,
    reveal: &str,
    warnings: Vec<symbrowse_core::output::Warning>,
) -> Result<String, String> {
    let origin = data
        .get("origin")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "cookie response has no origin".to_owned())?;
    let raw_cookies = data
        .get("cookies")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "cookie response has no cookies array".to_owned())?;
    let cookies = raw_cookies
        .iter()
        .map(|cookie| {
            let text = |key: &str| {
                cookie
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
            };
            let name = text("name");
            let value = text("value");
            CookieCliItem {
                name,
                value: if reveal_cookie(name, reveal) {
                    value.to_owned()
                } else {
                    masked_cookie_value(value)
                },
                domain: text("domain"),
                path: text("path"),
                expires: CookieCliNumber(
                    cookie
                        .get("expires")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(0.0),
                ),
                size: cookie
                    .get("size")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0),
                http_only: cookie
                    .get("http_only")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                secure: cookie
                    .get("secure")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                session: cookie
                    .get("session")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                same_site: (!text("same_site").is_empty()).then(|| text("same_site")),
            }
        })
        .collect();
    let envelope = CookieCliEnvelope {
        success: true,
        data: CookieCliData { origin, cookies },
        warnings,
    };
    serde_json::to_string(&envelope)
        .map(|mut output| {
            output.push('\n');
            output
        })
        .map_err(|error| error.to_string())
}

fn render_cookie_list_yaml(
    data: &serde_json::Value,
    warnings: Vec<symbrowse_core::output::Warning>,
) -> Result<String, String> {
    let origin = data
        .get("origin")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "cookie response has no origin".to_owned())?;
    let cookies = data
        .get("cookies")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "cookie response has no cookies array".to_owned())?;

    let mut output = format!(
        "success: true\ndata:\n    origin: {}\n",
        yaml_cli_string(origin)
    );
    if cookies.is_empty() {
        output.push_str("    cookies: []\n");
    } else {
        output.push_str("    cookies:\n");
        for cookie in cookies {
            let string = |key: &str| {
                cookie
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
            };
            let number = |key: &str| {
                cookie
                    .get(key)
                    .and_then(serde_json::Value::as_f64)
                    .map_or_else(|| "0".to_owned(), |value| value.to_string())
            };
            let integer = |key: &str| {
                cookie
                    .get(key)
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0)
            };
            let boolean = |key: &str| {
                cookie
                    .get(key)
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
            };
            output.push_str("        - name: ");
            output.push_str(&yaml_cli_string(string("name")));
            output.push('\n');
            for (key, value) in [
                ("value", yaml_cli_string(string("value"))),
                ("domain", yaml_cli_string(string("domain"))),
                ("path", yaml_cli_string(string("path"))),
                ("expires", number("expires")),
                ("size", integer("size").to_string()),
                ("httponly", boolean("http_only").to_string()),
                ("secure", boolean("secure").to_string()),
                ("session", boolean("session").to_string()),
                ("samesite", yaml_cli_string(string("same_site"))),
            ] {
                output.push_str("          ");
                output.push_str(key);
                output.push_str(": ");
                output.push_str(&value);
                output.push('\n');
            }
        }
    }

    let suffix = Envelope::ok(serde_json::Value::Null, warnings)
        .render(Format::Yaml)
        .map_err(|error| error.to_string())?;
    let suffix = suffix
        .strip_prefix("success: true\ndata: null\n")
        .ok_or_else(|| "could not render cookie YAML envelope suffix".to_owned())?;
    output.push_str(suffix);
    Ok(output)
}

fn yaml_cli_string(value: &str) -> String {
    if value.is_empty()
        || value.contains(['\n', '\r', '\t'])
        || value.contains(": ")
        || matches!(
            value.to_ascii_lowercase().as_str(),
            "y" | "yes" | "n" | "no" | "true" | "false" | "on" | "off" | "null" | "~"
        )
    {
        serde_json::to_string(value).expect("string serialization cannot fail")
    } else {
        value.to_owned()
    }
}

fn run_session_id(scope: &str, prefix: &str, format: Format) -> ExitCode {
    let info = match derive_session_id(scope, prefix) {
        Ok(info) => info,
        Err(message) => {
            if format == Format::Text {
                let _ = writeln!(io::stderr(), "{message}");
            } else if format == Format::Json {
                let output = format!(
                    "{{\"success\":false,\"error\":{{\"code\":\"internal\",\"message\":{}}}}}\n",
                    go_json_string(&message)
                );
                let _ = io::stdout().write_all(output.as_bytes());
            } else if let Ok(output) =
                Envelope::failure(ErrorCode::Internal, &message).render(format)
            {
                let _ = io::stdout().write_all(output.as_bytes());
            }
            return ExitCode::from(1);
        }
    };

    match format {
        Format::Text => write_stdout(&format!("{}\n", info.id)),
        Format::Json => write_stdout(&render_session_id_json(&info)),
        Format::Yaml => match serde_json::to_value(&info)
            .map(|data| Envelope::ok(data, Vec::new()).render(Format::Yaml))
        {
            Ok(Ok(output)) => write_stdout(&output),
            _ => ExitCode::from(1),
        },
    }
}

fn run_cache_get(id: String, range: Option<String>, format: Format) -> ExitCode {
    let context = match LoadContext::from_process(FlagOverrides::default()) {
        Ok(context) => context,
        Err(error) => return write_cache_get_error(error.to_string(), format),
    };
    let config = match load(&context) {
        Ok(result) => result.config,
        Err(error) => return write_cache_get_error(error.to_string(), format),
    };
    let cache_root = PathBuf::from(&config.cache_dir);
    let content = if let Some(key) = id.strip_prefix("fetch:") {
        let ttl_nanos = config.cache_ttl_hours.wrapping_mul(3_600_000_000_000);
        symbrowse_fetch::cache::ResponseCache::new(cache_root.join("fetch"))
            .load_key(key, ttl_nanos)
            .map_err(|error| match error {
                symbrowse_fetch::cache::CacheError::NotFound(_) => {
                    format!("fetch cache entry not found: {key}")
                }
                symbrowse_fetch::cache::CacheError::Expired(_) => {
                    format!("fetch cache entry expired: {key}")
                }
                other => other.to_string(),
            })
    } else {
        Cache::new(cache_root.join("out"), Duration::ZERO)
            .load(&id)
            .map_err(|error| error.to_string())
    };
    let content = match content {
        Ok(content) => content,
        Err(message) => return write_cache_get_error(message, format),
    };
    let content = if let Some(range) = range.as_deref().filter(|value| !value.trim().is_empty()) {
        let (start, end) = match parse_cache_range(range) {
            Ok(bounds) => bounds,
            Err(message) => return write_cache_get_error(message, format),
        };
        line_range_bytes(&content, start, end)
    } else {
        content
    };
    if format == Format::Text {
        let mut stdout = io::stdout().lock();
        if stdout.write_all(&content).is_err() || stdout.write_all(b"\n").is_err() {
            return ExitCode::from(1);
        }
        return ExitCode::SUCCESS;
    }
    let mut data = serde_json::Map::new();
    data.insert("cache_id".into(), serde_json::Value::String(id));
    data.insert(
        "content".into(),
        serde_json::Value::String(String::from_utf8_lossy(&content).into_owned()),
    );
    if let Some(range) = range.filter(|value| !value.trim().is_empty()) {
        data.insert("range".into(), serde_json::Value::String(range));
    }
    match Envelope::ok(serde_json::Value::Object(data), Vec::new()).render(format) {
        Ok(output) => write_stdout(&output),
        Err(error) => write_cache_get_error(error.to_string(), format),
    }
}

fn run_cache_clear(format: Format) -> ExitCode {
    let context = match LoadContext::from_process(FlagOverrides::default()) {
        Ok(context) => context,
        Err(error) => return write_cache_get_error(error.to_string(), format),
    };
    let config = match load(&context) {
        Ok(result) => result.config,
        Err(error) => return write_cache_get_error(error.to_string(), format),
    };
    let root = PathBuf::from(&config.cache_dir);
    let output_cache = Cache::new(root.join("out"), Duration::ZERO);
    let fetch_ttl = Duration::from_secs(
        u64::try_from(config.cache_ttl_hours.max(0))
            .unwrap_or(u64::MAX)
            .saturating_mul(3_600),
    );
    let fetch_cache =
        symbrowse_fetch::cache::ResponseCache::new(root.join("fetch")).with_ttl(fetch_ttl);
    let cleared = match clear_cache_entries(&output_cache, &fetch_cache, fetch_ttl) {
        Ok(count) => count,
        Err(error) => return write_cache_get_error(error, format),
    };
    if format == Format::Text {
        let suffix = if cleared == 1 { "y" } else { "ies" };
        return write_stdout(&format!("cleared {cleared} cache entr{suffix}\n"));
    }
    match Envelope::ok(serde_json::json!({"cleared": cleared}), Vec::new()).render(format) {
        Ok(output) => write_stdout(&output),
        Err(error) => write_cache_get_error(error.to_string(), format),
    }
}

#[derive(serde::Serialize)]
struct CacheListEntry {
    id: String,
    kind: String,
    bytes: u64,
    created_at: String,
    expires_at: String,
    expired: bool,
}

fn run_cache_list(format: Format) -> ExitCode {
    let context = match LoadContext::from_process(FlagOverrides::default()) {
        Ok(context) => context,
        Err(error) => return write_cache_get_error(error.to_string(), format),
    };
    let config = match load(&context) {
        Ok(result) => result.config,
        Err(error) => return write_cache_get_error(error.to_string(), format),
    };
    let root = PathBuf::from(config.cache_dir);
    let output_cache = Cache::new(root.join("out"), Duration::ZERO);
    let fetch_ttl = Duration::from_secs(
        u64::try_from(config.cache_ttl_hours.max(0))
            .unwrap_or(u64::MAX)
            .saturating_mul(3_600),
    );
    let entries = match cache_list_entries(&output_cache, &root.join("fetch"), fetch_ttl) {
        Ok(entries) => entries,
        Err(error) => return write_cache_get_error(error, format),
    };
    if format == Format::Text {
        if entries.is_empty() {
            return write_stdout("cache is empty\n");
        }
        let now = time::OffsetDateTime::now_utc();
        let mut output = String::new();
        for entry in entries {
            let created_at = time::OffsetDateTime::parse(
                &entry.created_at,
                &time::format_description::well_known::Rfc3339,
            );
            let age = created_at
                .map(|created_at| go_duration_seconds((now - created_at).whole_seconds()))
                .unwrap_or_else(|_| "0s".to_owned());
            let expiry = time::OffsetDateTime::parse(
                &entry.expires_at,
                &time::format_description::well_known::Rfc3339,
            )
            .ok()
            .filter(|expires_at| expires_at.year() != 1)
            .map(|expires_at| {
                format!(
                    "{} left",
                    go_duration_seconds((expires_at - now).whole_seconds())
                )
            })
            .unwrap_or_else(|| "never".to_owned());
            output.push_str(&format!(
                "{}\t{}\t{} bytes\t{} old\t{}\n",
                entry.id, entry.kind, entry.bytes, age, expiry
            ));
        }
        return write_stdout(&output);
    }
    match Envelope::ok(serde_json::json!({"entries": entries}), Vec::new()).render(format) {
        Ok(output) => write_stdout(&output),
        Err(error) => write_cache_get_error(error.to_string(), format),
    }
}

fn go_duration_seconds(seconds: i64) -> String {
    let negative = seconds < 0;
    let seconds = seconds.unsigned_abs();
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    let value = if hours > 0 {
        format!("{hours}h{minutes}m{seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m{seconds}s")
    } else {
        format!("{seconds}s")
    };
    if negative { format!("-{value}") } else { value }
}

fn clear_cache_entries(
    output_cache: &Cache,
    fetch_cache: &symbrowse_fetch::cache::ResponseCache,
    fetch_ttl: Duration,
) -> Result<usize, String> {
    let output_count = output_cache
        .list()
        .map_err(|error| error.to_string())?
        .len();
    let fetch_count = fetch_cache_entry_count(fetch_cache.root.as_path(), fetch_ttl)?;
    output_cache.clear().map_err(|error| error.to_string())?;
    fetch_cache.clear().map_err(|error| error.to_string())?;
    Ok(output_count + fetch_count)
}

fn fetch_cache_entry_count(root: &Path, default_ttl: Duration) -> Result<usize, String> {
    Ok(fetch_cache_entries(root, default_ttl)?.len())
}

fn cache_list_entries(
    output_cache: &Cache,
    fetch_root: &Path,
    default_ttl: Duration,
) -> Result<Vec<CacheListEntry>, String> {
    let mut entries = output_cache
        .list()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|entry| CacheListEntry {
            id: entry.id,
            kind: "output".to_owned(),
            bytes: entry.bytes,
            created_at: entry.created_at,
            expires_at: entry.expires_at,
            expired: entry.expired,
        })
        .collect::<Vec<_>>();
    entries.extend(fetch_cache_entries(fetch_root, default_ttl)?);
    entries.sort_by(|left, right| {
        let parse_time = |value: &str| {
            time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()
        };
        parse_time(&left.created_at)
            .cmp(&parse_time(&right.created_at))
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(entries)
}

fn fetch_cache_entries(root: &Path, default_ttl: Duration) -> Result<Vec<CacheListEntry>, String> {
    let directories = match fs::read_dir(root) {
        Ok(directories) => directories,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let now = time::OffsetDateTime::now_utc();
    let mut entries = Vec::new();
    for directory in directories.flatten().filter_map(|entry| {
        entry
            .file_type()
            .ok()
            .filter(|kind| kind.is_dir())
            .map(|_| entry.path())
    }) {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for metadata_path in entries.flatten().map(|entry| entry.path()).filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".meta.json"))
        }) {
            let Some(key) = metadata_path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".meta.json"))
            else {
                continue;
            };
            if key.len() != 64
                || !key
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            {
                continue;
            }
            let Ok(body_metadata) =
                fs::metadata(metadata_path.with_file_name(format!("{key}.body")))
            else {
                continue;
            };
            let Ok(raw) = fs::read(&metadata_path) else {
                continue;
            };
            let Ok(metadata) = serde_json::from_slice::<serde_json::Value>(&raw) else {
                continue;
            };
            let stored_at = match metadata.get("stored_at") {
                Some(serde_json::Value::String(value)) => time::OffsetDateTime::parse(
                    value,
                    &time::format_description::well_known::Rfc3339,
                )
                .ok(),
                Some(serde_json::Value::Null) | None => None,
                _ => continue,
            };
            // Go's Entries view uses the currently configured cache TTL,
            // matching the Rust fetch cache's load-time default.
            let ttl = default_ttl;
            let Some(stored_at) = stored_at else { continue };
            let age = (now - stored_at).whole_nanoseconds();
            if ttl.is_zero() || age <= ttl.as_nanos().min(i128::MAX as u128) as i128 {
                let expires_at = if ttl.is_zero() {
                    "0001-01-01T00:00:00Z".to_owned()
                } else {
                    let ttl_nanos = i64::try_from(ttl.as_nanos()).unwrap_or(i64::MAX);
                    stored_at
                        .checked_add(time::Duration::nanoseconds(ttl_nanos))
                        .and_then(|expires| {
                            expires
                                .format(&time::format_description::well_known::Rfc3339)
                                .ok()
                        })
                        .unwrap_or_else(|| "0001-01-01T00:00:00Z".to_owned())
                };
                entries.push(CacheListEntry {
                    id: format!("fetch:{key}"),
                    kind: "fetch-response".to_owned(),
                    bytes: body_metadata.len(),
                    created_at: stored_at
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_else(|_| "0001-01-01T00:00:00Z".to_owned()),
                    expires_at,
                    expired: false,
                });
            }
        }
    }
    Ok(entries)
}

fn parse_cache_range(spec: &str) -> Result<(usize, usize), String> {
    let spec = spec.trim();
    let (start, end) = spec.split_once('-').unwrap_or((spec, ""));
    let start = if start.is_empty() {
        0
    } else {
        start
            .parse::<usize>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                format!("invalid range {spec:?}: start must be a positive line number")
            })?
    };
    let end = if end.is_empty() {
        0
    } else {
        end.parse::<usize>()
            .ok()
            .filter(|value| *value >= start)
            .ok_or_else(|| format!("invalid range {spec:?}: end must be >= start"))?
    };
    Ok((start, end))
}

fn line_range_bytes(content: &[u8], start: usize, end: usize) -> Vec<u8> {
    let lines: Vec<_> = content.split(|byte| *byte == b'\n').collect();
    let first = start.max(1);
    if first > lines.len() {
        return Vec::new();
    }
    let last = if end < first || end == 0 {
        lines.len()
    } else {
        end.min(lines.len())
    };
    let mut result = Vec::new();
    for (index, line) in lines[first - 1..last].iter().enumerate() {
        if index > 0 {
            result.push(b'\n');
        }
        result.extend_from_slice(line);
    }
    result
}

fn write_cache_get_error(message: String, format: Format) -> ExitCode {
    if format == Format::Text {
        let _ = writeln!(io::stderr(), "{message}");
    } else if let Ok(output) = Envelope::failure(ErrorCode::Internal, &message).render(format) {
        let _ = io::stdout().write_all(output.as_bytes());
    }
    // The Go CLI classifies ordinary cache/config errors as ExitGeneric (1).
    ExitCode::from(1)
}

fn render_session_id_json(info: &SessionIdInfo) -> String {
    let mut output = format!(
        "{{\"success\":true,\"data\":{{\"id\":{},\"scope\":{}",
        go_json_string(&info.id),
        go_json_string(&info.scope)
    );
    if !info.prefix.is_empty() {
        output.push_str(&format!(",\"prefix\":{}", go_json_string(&info.prefix)));
    }
    output.push_str(&format!(
        ",\"origin_path\":{}",
        go_json_string(&info.origin_path)
    ));
    if info.fallback {
        output.push_str(",\"fallback\":true");
    }
    output.push_str("}}\n");
    output
}

fn derive_session_id(scope: &str, prefix: &str) -> Result<SessionIdInfo, String> {
    if !matches!(scope, "worktree" | "repo" | "cwd") {
        return Err(format!("invalid scope {scope:?}"));
    }
    let cwd = session_working_directory()
        .map_err(|error| format!("determine working directory: {error}"))?;
    if scope == "cwd" {
        return Ok(session_id_info(scope, prefix, &cwd, true));
    }

    let Some(top_level) = git_output(&cwd, &["rev-parse", "--show-toplevel"]) else {
        return Ok(session_id_info(scope, prefix, &cwd, true));
    };
    let worktree = clean_absolute_path(Path::new(top_level.trim()));
    let Some(common_output) = git_output(&cwd, &["rev-parse", "--git-common-dir"]) else {
        return Ok(session_id_info(scope, prefix, &worktree, false));
    };
    let common = PathBuf::from(common_output.trim());
    let common = if common.is_absolute() {
        common
    } else {
        worktree.join(common)
    };
    let common = clean_absolute_path(&common);
    let repository = if common.file_name().is_some_and(|name| name == ".git") {
        common.parent().unwrap_or(&common).to_path_buf()
    } else {
        common
    };
    let anchor = if scope == "repo" {
        repository
    } else {
        worktree
    };
    Ok(session_id_info(scope, prefix, &anchor, false))
}

fn session_working_directory() -> std::io::Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    #[cfg(unix)]
    if let Some(logical) = std::env::var_os("PWD").map(PathBuf::from)
        && logical.is_absolute()
        && let (Ok(logical_real), Ok(cwd_real)) =
            (std::fs::canonicalize(&logical), std::fs::canonicalize(&cwd))
        && logical_real == cwd_real
    {
        return Ok(clean_absolute_path(&logical));
    }
    Ok(clean_absolute_path(&cwd))
}

fn session_id_info(scope: &str, prefix: &str, anchor: &Path, fallback: bool) -> SessionIdInfo {
    let origin_path = normalized_path_string(anchor);
    let digest = Sha256::digest(origin_path.as_bytes());
    let short_hash = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let id = if prefix.is_empty() {
        short_hash
    } else {
        format!("{prefix}-{short_hash}")
    };
    SessionIdInfo {
        id,
        scope: scope.to_owned(),
        prefix: prefix.to_owned(),
        origin_path,
        fallback,
    }
}

fn git_output(directory: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn clean_absolute_path(path: &Path) -> PathBuf {
    let mut cleaned = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !cleaned.pop() && !path.has_root() {
                    cleaned.push("..");
                }
            }
            other => cleaned.push(other.as_os_str()),
        }
    }
    cleaned
}

fn normalized_path_string(path: &Path) -> String {
    let path = path.to_string_lossy().into_owned();
    #[cfg(windows)]
    {
        path.replace('/', "\\")
    }
    #[cfg(not(windows))]
    {
        path
    }
}

fn go_json_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{0008}' => output.push_str("\\b"),
            '\u{000c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{2028}' => output.push_str("\\u2028"),
            '\u{2029}' => output.push_str("\\u2029"),
            character if character <= '\u{001f}' => {
                output.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

fn run_storage_get(session: String, kind: String, key: Option<String>, format: Format) -> ExitCode {
    let response = Client::new(ClientOptions {
        socket_path: default_socket_path(&session),
        session: session.clone(),
        ..ClientOptions::default()
    })
    .request(Frame {
        cmd: "storage.list".to_owned(),
        args: Some(serde_json::json!({"kind": kind})),
        session,
        ..Frame::default()
    });
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            return render_dispatch_error(
                format,
                daemon_codes::DAEMON_UNAVAILABLE,
                error.to_string(),
            );
        }
    };
    if !response.success {
        let error = response.error.unwrap_or_default();
        return render_dispatch_error(format, &error.code, error.message);
    }
    let data = response.data.unwrap_or(serde_json::Value::Null);
    if format != Format::Text {
        let envelope = Envelope::ok(
            data,
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
        return match envelope.render(format) {
            Ok(output) => write_stdout(&output),
            Err(error) => {
                render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error.to_string())
            }
        };
    }
    let origin = data
        .get("origin")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let items = data.get("items").and_then(serde_json::Value::as_object);
    if let Some(key) = key {
        let Some(value) = items
            .and_then(|items| items.get(&key))
            .and_then(serde_json::Value::as_str)
        else {
            let _ = writeln!(
                io::stderr(),
                "key {key:?} not found in {kind} storage of {origin}"
            );
            return ExitCode::from(1);
        };
        return write_stdout(&(value.to_owned() + "\n"));
    }
    let output = items
        .into_iter()
        .flat_map(serde_json::Map::iter)
        .filter_map(|(key, value)| value.as_str().map(|value| format!("{key}\t{value}\n")))
        .collect::<String>();
    write_stdout(&output)
}

fn parse_eval(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut session = String::from("default");
    let mut expression = None;
    let mut from_stdin = false;
    let mut base64 = false;
    let mut index = command_index + 1;
    let mut positional_only = false;
    while index < values.len() {
        let value = &values[index];
        if positional_only {
            expression.get_or_insert_with(|| value.clone());
            index += 1;
            continue;
        }
        match value.as_str() {
            "--" => positional_only = true,
            "--stdin" => from_stdin = true,
            "--base64" | "-b" => base64 = true,
            "--json" => json = true,
            _ if value.starts_with("--stdin=") => {
                from_stdin = parse_bool("--stdin", &value[8..])?;
            }
            _ if value.starts_with("--base64=") => {
                base64 = parse_bool("--base64", &value[9..])?;
            }
            _ if value.starts_with("-b=") => base64 = parse_bool("--base64", &value[3..])?,
            _ if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            _ if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            _ if value.starts_with("--session=") => session = value[10..].to_owned(),
            _ if value.starts_with('-') => {
                return Err(unknown_flag(value));
            }
            _ => {
                expression.get_or_insert_with(|| value.clone());
            }
        }
        index += 1;
    }
    if json {
        format = Format::Json;
    }
    Ok(Action::Eval {
        session,
        expression,
        from_stdin,
        base64,
        format,
    })
}

fn root_output_flags(values: &[String]) -> Result<(Format, bool), ParseError> {
    let mut format = Format::Text;
    let mut json = false;
    let mut index = 0;
    while index < values.len() {
        match values[index].as_str() {
            "--" => break,
            "--json" => json = true,
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            _ => {}
        }
        index += 1;
    }
    Ok((format, json))
}

fn run_eval(
    session: String,
    expression: Option<String>,
    from_stdin: bool,
    base64_encoded: bool,
    format: Format,
) -> ExitCode {
    let stdin_bytes = if from_stdin {
        let mut bytes = Vec::new();
        if let Err(error) = io::stdin().read_to_end(&mut bytes) {
            return render_dispatch_error(
                format,
                "invalid_args",
                format!("read expression from stdin: {error}"),
            );
        }
        bytes
    } else {
        Vec::new()
    };
    let expression = match resolve_eval_expression(expression, from_stdin, &stdin_bytes) {
        Some(expression) => expression,
        None => {
            return render_dispatch_error(
                format,
                "invalid_args",
                "eval requires an expression argument (or --stdin)".into(),
            );
        }
    };
    let expression = if base64_encoded {
        match decode_standard_base64(&expression) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(index) => {
                return render_dispatch_error(
                    format,
                    "invalid_args",
                    format!("decode base64 expression: illegal base64 data at input byte {index}"),
                );
            }
        }
    } else {
        expression
    };
    let frame = Frame {
        cmd: "eval".into(),
        args: Some(serde_json::json!({"expression": expression})),
        session: session.clone(),
        ..Frame::default()
    };
    let response = match Client::new(ClientOptions {
        socket_path: default_socket_path(&session),
        session,
        ..ClientOptions::default()
    })
    .request(frame)
    {
        Ok(response) => response,
        Err(error) => {
            return render_dispatch_error(
                format,
                daemon_codes::DAEMON_UNAVAILABLE,
                error.to_string(),
            );
        }
    };
    if !response.success {
        let error = response.error.unwrap_or_default();
        return render_dispatch_error(format, &error.code, error.message);
    }
    let data = response.data.unwrap_or(serde_json::Value::Null);
    if format != Format::Text {
        return match Envelope::ok(data, Vec::new()).render(format) {
            Ok(output) => write_stdout(&output),
            Err(error) => {
                render_dispatch_error(format, daemon_codes::OPERATION_FAILED, error.to_string())
            }
        };
    }
    if let Some(exception) = data
        .get("exception_text")
        .and_then(serde_json::Value::as_str)
        && !exception.is_empty()
    {
        let _ = writeln!(io::stderr(), "eval threw: {exception}");
        return ExitCode::FAILURE;
    }
    let value = data
        .get("value")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let output = if value.is_null() {
        "undefined".to_owned()
    } else {
        value.to_string()
    };
    let _ = writeln!(io::stdout(), "{output}");
    ExitCode::SUCCESS
}

fn resolve_eval_expression(
    expression: Option<String>,
    from_stdin: bool,
    stdin_bytes: &[u8],
) -> Option<String> {
    if from_stdin {
        Some(String::from_utf8_lossy(stdin_bytes).into_owned())
    } else {
        expression
    }
}

fn decode_standard_base64(input: &str) -> Result<Vec<u8>, usize> {
    let mut filtered = Vec::with_capacity(input.len());
    let mut offsets = Vec::with_capacity(input.len());
    for (index, byte) in input.bytes().enumerate() {
        if byte != b'\r' && byte != b'\n' {
            filtered.push(byte);
            offsets.push(index);
        }
    }
    GO_STANDARD_BASE64.decode(&filtered).map_err(|error| {
        use base64::DecodeError;
        let decoded_index = match error {
            DecodeError::InvalidByte(index, b'=')
                if let Some(end) = padded_group_end(&filtered, index) =>
            {
                end
            }
            DecodeError::InvalidByte(index, _) | DecodeError::InvalidLastSymbol(index, _) => index,
            DecodeError::InvalidLength(length) => length.saturating_sub(length % 4),
            DecodeError::InvalidPadding if filtered.last() == Some(&b'=') => filtered.len(),
            DecodeError::InvalidPadding => filtered.len().saturating_sub(filtered.len() % 4),
        };
        offsets.get(decoded_index).copied().unwrap_or(input.len())
    })
}

fn padded_group_end(input: &[u8], padding_index: usize) -> Option<usize> {
    let group_start = padding_index.checked_sub(padding_index % 4)?;
    let group = input.get(group_start..group_start + 4)?;
    let canonical_padding =
        (group[2] == b'=' && group[3] == b'=') || (group[3] == b'=' && group[2] != b'=');
    (canonical_padding && group_start + 4 < input.len()).then_some(group_start + 4)
}

fn error_code(code: &str) -> ErrorCode {
    match code {
        "internal" => ErrorCode::Internal,
        "invalid_args" => ErrorCode::InvalidArgs,
        daemon_codes::MALFORMED_REQUEST => ErrorCode::MalformedRequest,
        daemon_codes::UNKNOWN_COMMAND => ErrorCode::UnknownCommand,
        daemon_codes::OPERATION_TIMEOUT => ErrorCode::OperationTimeout,
        daemon_codes::PEER_DENIED => ErrorCode::PeerDenied,
        daemon_codes::DAEMON_UNAVAILABLE => ErrorCode::DaemonUnavailable,
        daemon_codes::INVALID_SESSION => ErrorCode::InvalidSession,
        _ => ErrorCode::OperationFailed,
    }
}

fn render_dispatch_error(format: Format, code: &str, message: String) -> ExitCode {
    let mapped = error_code(code);
    if format == Format::Text {
        let _ = writeln!(io::stderr(), "{message}");
    } else {
        render_envelope_error(Envelope::failure(mapped, message), format);
    }
    ExitCode::from(mapped.exit_code())
}

fn render_client_error(format: Format, error: ClientError) -> ExitCode {
    match error {
        ClientError::Transport(error) => render_daemon_error(format, error),
        ClientError::Io(error) => {
            render_dispatch_error(format, daemon_codes::DAEMON_UNAVAILABLE, error.to_string())
        }
        ClientError::Unsupported => render_dispatch_error(
            format,
            daemon_codes::DAEMON_UNAVAILABLE,
            "daemon sockets are not supported on this platform".to_owned(),
        ),
    }
}

fn render_daemon_error(format: Format, error: DaemonError) -> ExitCode {
    let mapped = error_code(&error.code);
    if format == Format::Text {
        let _ = writeln!(io::stderr(), "{}", error.message);
    } else {
        let mut envelope = Envelope::failure(mapped, error.message);
        if let Some(payload) = envelope.error.as_mut() {
            payload.hint = error.hint;
            payload.details = error.details;
            payload.retryable = error.retryable;
            payload.requires_user_confirmation = error.requires_user_confirmation;
            payload.resume_hint = error.resume_hint;
        }
        render_envelope_error(envelope, format);
    }
    ExitCode::from(mapped.exit_code())
}

fn render_envelope_error(envelope: Envelope, format: Format) {
    match envelope.render(format) {
        Ok(output) => {
            let _ = io::stdout().write_all(output.as_bytes());
        }
        Err(_) => {
            let _ = writeln!(io::stderr(), "dispatch failed");
        }
    }
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
    if matches!(
        value,
        "chrome" | "firefox" | "static" | "safari-attach" | "safari-bidi"
    ) {
        Ok(())
    } else {
        Err(format!(
            "invalid engine {value:?}: use one of chrome, firefox, static, safari-attach, safari-bidi"
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
    let engine_flag = !engine.is_empty();
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
    let selection_engine = (mode == "browser" || engine_flag).then_some(engine.as_str());
    if let Err(error) = symbrowse_core::config::resolve_selection(Some(&mode), selection_engine) {
        let _ = writeln!(
            io::stderr(),
            "invalid transport selection: {}",
            error.message
        );
        return ExitCode::from(1);
    }
    let engine = if mode != "browser" {
        "static".to_owned()
    } else {
        engine
    };
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
        return match render_batch_text(&report) {
            Ok(output) => write_stdout(&output),
            Err(_) => ExitCode::from(1),
        };
    }
    write_stdout(&batch::render_yaml(&report))
}

fn render_batch_text(report: &batch::Report) -> Result<String, serde_json::Error> {
    let mut output = go_json_html_escape(serde_json::to_string_pretty(report)?);
    output.push('\n');
    Ok(output)
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

fn execute_batch_daemon_lifecycle(session: String, command: String, format: Format) -> ItemOutput {
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
            return ItemOutput {
                stdout: String::new(),
                error: Some(error.to_string()),
            };
        }
    };
    if !response.success {
        return ItemOutput {
            stdout: String::new(),
            error: Some(
                response
                    .error
                    .map_or_else(|| "daemon request failed".to_owned(), |error| error.message),
            ),
        };
    }
    let stdout = if format == Format::Text {
        response.data.as_ref().map_or_else(
            || "ok\n".to_owned(),
            |data| {
                serde_json::to_string_pretty(data)
                    .map_or_else(|_| "ok\n".to_owned(), |text| format!("{text}\n"))
            },
        )
    } else {
        let warnings = response
            .warnings
            .into_iter()
            .map(|warning| symbrowse_core::output::Warning {
                kind: warning.kind,
                severity: warning.severity,
                message: warning.message,
                r#ref: warning.r#ref,
                excerpt: warning.excerpt,
            })
            .collect();
        match (Envelope::ok(response.data.unwrap_or(serde_json::Value::Null), warnings))
            .render(format)
        {
            Ok(output) => output,
            Err(error) => {
                return ItemOutput {
                    stdout: String::new(),
                    error: Some(error.to_string()),
                };
            }
        }
    };
    ItemOutput {
        stdout,
        error: None,
    }
}

fn execute_batch_state_operation(
    session: String,
    command: String,
    name: Option<String>,
    older_than: Option<i64>,
    format: Format,
) -> ItemOutput {
    let args = match (name.as_deref(), older_than) {
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
            return ItemOutput {
                stdout: String::new(),
                error: Some(error.to_string()),
            };
        }
    };
    if !response.success {
        return ItemOutput {
            stdout: String::new(),
            error: Some(
                response
                    .error
                    .map_or_else(|| "state request failed".to_owned(), |error| error.message),
            ),
        };
    }
    let stdout = if format == Format::Text {
        let data = response.data.unwrap_or(serde_json::Value::Null);
        match command.as_str() {
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
                .map(|states| {
                    states
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(|state| format!("{state}\n"))
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
        }
    } else {
        let warnings = response
            .warnings
            .into_iter()
            .map(|warning| symbrowse_core::output::Warning {
                kind: warning.kind,
                severity: warning.severity,
                message: warning.message,
                r#ref: warning.r#ref,
                excerpt: warning.excerpt,
            })
            .collect();
        match Envelope::ok(response.data.unwrap_or(serde_json::Value::Null), warnings)
            .render(format)
        {
            Ok(output) => output,
            Err(error) => {
                return ItemOutput {
                    stdout: String::new(),
                    error: Some(error.to_string()),
                };
            }
        }
    };
    ItemOutput {
        stdout,
        error: None,
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
        Ok(Action::DaemonLifecycle {
            session,
            command,
            format,
        }) => execute_batch_daemon_lifecycle(session, command, format),
        Ok(Action::StateOperation {
            session,
            command,
            name,
            older_than,
            format,
        }) => execute_batch_state_operation(session, command, name, older_than, format),
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
                selection: explicit_selection(&result),
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
        let selection_code = message.split(':').nth(1).map(str::trim).filter(|code| {
            matches!(
                *code,
                "invalid_transport_mode"
                    | "invalid_browser_engine"
                    | "engine_not_allowed"
                    | "browser_engine_required"
            )
        });
        let envelope = if let Some(code) = selection_code {
            let mut envelope =
                Envelope::failure(ErrorCode::Validation, "invalid transport selection");
            if let Some(error) = &mut envelope.error {
                error.details = Some(serde_json::json!({"selection_code": code}));
            }
            envelope
        } else {
            let envelope_message = message.split_once(':').map_or(message, |(outer, _)| outer);
            Envelope::failure(ErrorCode::Config, envelope_message)
        };
        if let Ok(output) = envelope.render(format) {
            let _ = io::stdout().write_all(output.as_bytes());
        }
    }
    ExitCode::from(9)
}

fn parse(args: &[OsString]) -> Result<Action, ParseError> {
    let values: Vec<String> = args
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    if values.first().is_some_and(|value| value == "__complete") {
        return Ok(Action::CompletionRequest {
            args: values[1..].to_vec(),
        });
    }
    if values.first().is_some_and(|value| value == "completion") {
        return parse_completion(&values);
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
        let suffix = if matches!(command, "storage" | "session") {
            let mut suffix = Vec::new();
            let mut skip_value = false;
            for value in &values[command_index + 1..help_index] {
                if skip_value {
                    skip_value = false;
                } else if value == "--session" {
                    skip_value = true;
                } else if !value.starts_with('-') {
                    suffix.push(value.as_str());
                }
            }
            suffix
        } else {
            values[command_index + 1..help_index]
                .iter()
                .filter(|value| !value.starts_with('-'))
                .map(String::as_str)
                .collect::<Vec<_>>()
        };
        if let Some(text) = help_catalog::help(command, &suffix) {
            return Ok(Action::Help(text.to_owned()));
        }
        if let Some(text) = command_help(command, &suffix) {
            return Ok(Action::Help(text));
        }
        if command == "help" {
            return parse_help(&values, command_index);
        }
    }
    let Some(command_index) = top_command_index(&values) else {
        if let Some(value) = values.iter().find(|value| value.starts_with('-')) {
            return Err(unknown_flag(value));
        }
        return Err(ParseError {
            message: "a command is required".to_owned(),
            exit_code: 2,
        });
    };
    match values[command_index].as_str() {
        "handoff" => parse_handoff(&values, command_index),
        "oob" => parse_oob(&values, command_index),
        "auth" => parse_auth(&values, command_index),
        "version" => parse_version(&values, command_index),
        "upgrade" => parse_upgrade(&values, command_index),
        "doctor" => parse_doctor(&values, command_index),
        "downloads" => parse_downloads(&values, command_index),
        "console" | "errors" => parse_runtime_events(&values, command_index),
        "eval" => parse_eval(&values, command_index),
        "config" => parse_config(&values, command_index),
        "batch" => parse_batch(&values, command_index),
        "state" => parse_state_lifecycle(&values, command_index),
        "daemon" => parse_daemon(&values, command_index),
        "mcp" => parse_mcp(&values, command_index),
        "flow" | "workflow" => parse_flow(&values, command_index),
        "dialog" => parse_dialog(&values, command_index),
        "tab" => parse_tab(&values, command_index),
        "frame" => parse_frame(&values, command_index),
        "storage" => parse_storage(&values, command_index),
        "cookies" => parse_cookies(&values, command_index),
        "session" => parse_session(&values, command_index),
        "cache" => parse_cache(&values, command_index),
        "set" => parse_set(&values, command_index),
        "policy" => parse_policy(&values, command_index),
        "journal" => parse_journal(&values, command_index),
        "watch" => parse_watch(&values, command_index),
        "trace" => parse_trace(&values, command_index),
        "diff" => parse_diff(&values, command_index),
        "network" => parse_network(&values, command_index),
        "profiles" => {
            let mut arguments = values;
            arguments.remove(command_index);
            Ok(Action::ProfileList { arguments })
        }
        "tools" => parse_tools(&values, command_index),
        "help" => parse_help(&values, command_index),
        "compat-sidecar" => parse_compat_sidecar(&values, command_index),
        "fetch" | "read" | "open" | "goto" | "snapshot" | "click" | "fill" | "type" | "press"
        | "wait" | "back" | "forward" | "reload" | "get" | "is" | "find" | "check" | "dblclick"
        | "focus" | "hover" | "select" | "uncheck" | "scroll" | "scrollintoview" | "a11y"
        | "screenshot" | "upload" => parse_dispatch(&values, command_index),
        command => Err(ParseError {
            message: format!("unknown command {command:?} for \"symbrowse\""),
            exit_code: 2,
        }),
    }
}

fn parse_compat_sidecar(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let _ = root_output_flags(&values[..command_index])?;
    let mut index = command_index + 1;
    while index < values.len() {
        match values[index].as_str() {
            "--json" => {}
            value if value.starts_with("--json=") => {
                parse_bool("--json", &value[7..])?;
            }
            "--output" => {
                index += 1;
                parse_format(required_value(values, index, "--output")?)?;
            }
            value if value.starts_with("--output=") => {
                parse_format(&value[9..])?;
            }
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            extra => {
                return Err(ParseError {
                    message: format!("unknown command {extra:?} for \"symbrowse compat-sidecar\""),
                    exit_code: 2,
                });
            }
        }
        index += 1;
    }
    Ok(Action::CompatSidecar)
}

fn run_compat_sidecar() -> ExitCode {
    let Some(binary) = std::env::var_os("SYMBROWSE_COMPAT_BINARY") else {
        let _ = writeln!(
            io::stderr(),
            "compat-sidecar requires SYMBROWSE_COMPAT_BINARY to name the retained Go helper"
        );
        return ExitCode::from(1);
    };
    let binary = PathBuf::from(binary);
    if !binary.is_absolute() {
        let _ = writeln!(
            io::stderr(),
            "compat-sidecar requires SYMBROWSE_COMPAT_BINARY to be an absolute path"
        );
        return ExitCode::from(1);
    }
    if let (Ok(current), Ok(helper)) = (
        std::env::current_exe().and_then(fs::canonicalize),
        fs::canonicalize(&binary),
    ) && current == helper
    {
        let _ = writeln!(
            io::stderr(),
            "compat-sidecar cannot use the symbrowse Rust executable as its helper"
        );
        return ExitCode::from(1);
    }
    let status = Command::new(binary)
        .arg("compat-sidecar")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
    match status {
        Ok(status) => status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .map_or_else(|| ExitCode::from(1), ExitCode::from),
        Err(_) => {
            let _ = writeln!(io::stderr(), "compat-sidecar helper could not be started");
            ExitCode::from(1)
        }
    }
}

fn parse_completion(values: &[String]) -> Result<Action, ParseError> {
    if values
        .iter()
        .skip(1)
        .any(|value| matches!(value.as_str(), "-h" | "--help"))
    {
        return Ok(Action::Help(completion_help().to_owned()));
    }
    if values.len() != 2 {
        return Err(ParseError {
            message: "completion requires one shell: bash, zsh, fish or powershell".to_owned(),
            exit_code: 2,
        });
    }
    let shell = values[1].as_str();
    if completion::script(shell).is_none() {
        return Err(ParseError {
            message: format!("unsupported shell {shell:?}; choose bash, zsh, fish or powershell"),
            exit_code: 2,
        });
    }
    Ok(Action::Completion {
        shell: shell.to_owned(),
    })
}

fn completion_help() -> &'static str {
    "Generate the autocompletion script for symbrowse for the specified shell.\nSee each sub-command's help for details on how to use the generated script.\n\nUsage:\n  symbrowse completion [command]\n\nAvailable Commands:\n  bash        Generate the autocompletion script for bash\n  fish        Generate the autocompletion script for fish\n  powershell  Generate the autocompletion script for powershell\n  zsh         Generate the autocompletion script for zsh\n\nFlags:\n  -h, --help   help for completion\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n\nUse \"symbrowse completion [command] --help\" for more information about a command.\n"
}

fn parse_doctor(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let mut format = Format::Text;
    let mut fix = false;
    let mut index = 0;
    while index < values.len() {
        let value = &values[index];
        if index == command_index {
            index += 1;
            continue;
        }
        match value.as_str() {
            "--fix" => fix = true,
            value if value.starts_with("--fix=") => fix = parse_bool("--fix", &value[6..])?,
            "--json" => format = Format::Json,
            value if value.starts_with("--json=") => {
                if parse_bool("--json", &value[7..])? {
                    format = Format::Json;
                }
            }
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            value if value.starts_with("--output=") => {
                format = parse_format(&value[9..])?;
            }
            "--" => {
                if let Some(extra) = values.get(index + 1) {
                    return Err(ParseError {
                        message: format!("unknown command {extra:?} for \"symbrowse doctor\""),
                        exit_code: 2,
                    });
                }
                break;
            }
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            extra => {
                return Err(ParseError {
                    message: format!("unknown command {extra:?} for \"symbrowse doctor\""),
                    exit_code: 2,
                });
            }
        }
        index += 1;
    }
    Ok(Action::Doctor { format, fix })
}

fn parse_downloads(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut session = String::from("default");
    let mut dir = None;
    let mut index = command_index + 1;
    while index < values.len() {
        let value = &values[index];
        match value.as_str() {
            "--json" => json = true,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            "--session" | "--dir" => {
                let flag = value.clone();
                index += 1;
                let argument = required_value(values, index, &flag)?;
                if flag == "--session" {
                    session = argument.to_owned();
                } else {
                    dir = Some(argument.to_owned());
                }
            }
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with("--dir=") => dir = Some(value[6..].to_owned()),
            "--" => {
                if let Some(extra) = values.get(index + 1) {
                    return Err(ParseError {
                        message: format!("unknown command {extra:?} for \"symbrowse downloads\""),
                        exit_code: 2,
                    });
                }
                break;
            }
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            extra => {
                return Err(ParseError {
                    message: format!("unknown command {extra:?} for \"symbrowse downloads\""),
                    exit_code: 2,
                });
            }
        }
        index += 1;
    }
    if json {
        format = Format::Json;
    }
    Ok(Action::Downloads {
        session,
        dir,
        format,
    })
}

fn parse_runtime_events(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let resource = values[command_index].as_str();
    let command = values
        .get(command_index + 1)
        .map(String::as_str)
        .unwrap_or("");
    if !matches!(command, "list" | "clear") {
        return Err(ParseError {
            message: format!("{resource} requires list or clear"),
            exit_code: 2,
        });
    }
    let mut session = "default".to_owned();
    let mut max_tokens = None;
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut index = command_index + 2;
    while index < values.len() {
        match values[index].as_str() {
            "--json" => json = true,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            "--max-tokens" if command == "list" => {
                index += 1;
                let value = required_value(values, index, "--max-tokens")?
                    .parse::<i64>()
                    .map_err(|_| ParseError {
                        message: "invalid max-tokens value".to_owned(),
                        exit_code: 2,
                    })?;
                max_tokens = (value > 0).then_some(value);
            }
            value if value.starts_with("--json=") => {
                json = parse_bool("--json", &value[7..])?;
            }
            value if value.starts_with("--output=") => {
                format = parse_format(&value[9..])?;
            }
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with("--max-tokens=") && command == "list" => {
                let parsed = value[13..].parse::<i64>().map_err(|_| ParseError {
                    message: "invalid max-tokens value".to_owned(),
                    exit_code: 2,
                })?;
                max_tokens = (parsed > 0).then_some(parsed);
            }
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            value => {
                return Err(ParseError {
                    message: format!("unknown argument {value:?}"),
                    exit_code: 2,
                });
            }
        }
        index += 1;
    }
    if json {
        format = Format::Json;
    }
    Ok(Action::RuntimeEvents {
        session,
        command: format!("{resource}.{command}"),
        max_tokens,
        format,
    })
}

fn parse_help(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let path = values[command_index + 1..]
        .iter()
        .take_while(|value| value.as_str() != "--")
        .filter(|value| !value.starts_with('-'))
        .map(String::as_str)
        .collect::<Vec<_>>();
    let Some((target, suffix)) = path.split_first() else {
        if values[command_index + 1..]
            .iter()
            .any(|value| matches!(value.as_str(), "-h" | "--help"))
        {
            return Ok(Action::Help(help_command_help().to_owned()));
        }
        return Ok(Action::Help(root_help()));
    };
    if *target == "help" {
        return Ok(Action::Help(help_command_help().to_owned()));
    }
    if let Some(text) = help_catalog::help(target, suffix) {
        return Ok(Action::Help(text.to_owned()));
    }
    if let Some(text) = command_help(target, suffix) {
        return Ok(Action::Help(text));
    }
    Ok(Action::Help(root_help()))
}

fn help_command_help() -> &'static str {
    "Help provides help for any command in the application.\nSimply type symbrowse help [path to command] for full details.\n\nUsage:\n  symbrowse help [command] [flags]\n\nFlags:\n  -h, --help   help for help\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n"
}

fn root_help() -> String {
    "symbrowse is the standalone command-line entrypoint for Symaira Browse.\n\nUsage:\n  symbrowse [command]\n\nCore Commands:\n  batch          Run multiple commands in one process and report per-item status\n  check          Check a checkbox or radio element\n  click          Click an element matching a selector or @ref\n  dblclick       Double-click an element matching a selector or @ref\n  fill           Fill an input element, replacing its content\n  find           Find an element semantically and optionally act on it\n  focus          Focus an element matching a selector or @ref\n  get            Inspect page and element values\n  goto           Navigate to a URL (alias for open)\n  hover          Hover over an element matching a selector or @ref\n  is             Check page and element state\n  open           Open a URL in the browser and wait for load\n  press          Press a keyboard key on an element\n  read           Render the page as markdown (or JSON) in the symfetch output schema\n  screenshot     Capture the page (viewport, --full page, or --selector element)\n  scroll         Scroll the page or an element by pixel amount\n  scrollintoview Scroll an element into the visible viewport\n  select         Select an option from a drop-down element\n  snapshot       Render the accessibility tree\n  type           Type text into an element, appending to its content\n  uncheck        Uncheck a checkbox element\n  wait           Wait for a browser condition\n\nNavigation Commands:\n  back           Navigate back in page history\n  dialog         Handle JavaScript dialogs (accept, dismiss, status, auto)\n  forward        Navigate forward in page history\n  frame          Address nested frames (tree, select, main)\n  reload         Reload the current page\n  tab            Manage session tabs (list, new, switch, close)\n\nState Commands:\n  auth           Credential management through symvault (no plaintext)\n  cookies        Inspect and manage cookies of the current page origin\n  handoff        Hand the session over to the human without losing it (2FA, CAPTCHA, approval)\n  journal        Inspect the append-only action journal\n  oob            Inspect the out-of-band human channel\n  profiles       List discovered Chrome profiles available for reuse\n  session        Inspect browser sessions\n  set            Apply session-wide emulation settings (viewport, device, geo, offline, headers, media, user-agent)\n  state          Save, restore and manage named browser session states\n  storage        Inspect and manage per-origin web storage\n  watch          Watch an agent session: stream the action journal live (read-only)\n\nNetwork Commands:\n  downloads      Show download events (origin URL, size, checksum) or set the download directory\n  network        Inspect, mock and export page network activity\n  upload         Upload files into a file input (path-guarded)\n\nDebug Commands:\n  a11y           Run an axe-core accessibility audit on the current page\n  cache          Inspect the truncate-and-store output cache\n  compat-sidecar Run the pinned Go/AzureTLS compatibility sidecar\n  config         Inspect symbrowse configuration\n  console        Show or clear the page console buffer\n  daemon         Run or inspect the symbrowse daemon\n  diff           Compare snapshots, screenshots and URLs\n  doctor         Check browser discovery and local runtime prerequisites\n  errors         Show or clear uncaught page errors\n  eval           Execute JavaScript in the active page\n  mcp            Start the MCP stdio server (JSON-RPC 2.0 over stdin/stdout)\n  policy         Inspect the local risk policy\n  trace          Export and replay repeatable action traces\n  upgrade        Check for and apply symbrowse updates\n  version        Print the symbrowse version\n\nFlows Commands:\n  flow           Validate, run and record declarative browser flows\n\nAdditional Commands:\n  completion     Generate the autocompletion script for the specified shell\n  help           Help about any command\n\nFlags:\n  -h, --help            help for symbrowse\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n  -v, --version         version for symbrowse\n\nUse \"symbrowse [command] --help\" for more information about a command.\n".to_owned()
}

fn command_help(command: &str, suffix: &[&str]) -> Option<String> {
    let first = suffix.first().copied();
    let target = match command {
        "workflow" => "flow",
        other => other,
    };
    if matches!(
        target,
        "check"
            | "dblclick"
            | "focus"
            | "hover"
            | "select"
            | "uncheck"
            | "scroll"
            | "scrollintoview"
    ) && first.is_none()
    {
        return Some(interaction_help(target));
    }
    let global = "Global Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n";
    let session_global = "Global Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --session string   session name (default \"default\")\n";
    let plain = |description: &str, usage: &str, flags: &str, globals: &str| {
        format!("{description}\n\nUsage:\n  {usage}\n\nFlags:\n{flags}\n{globals}")
    };
    match (target, first) {
        ("auth", None) => Some(
            "Credential management through symvault (no plaintext)\n\nUsage:\n  symbrowse auth [command]\n\nAvailable Commands:\n  login       Resolve a vault entry and type the credentials into the detected login form\n\nFlags:\n  -h, --help             help for auth\n      --session string   session name (default \"default\")\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n\nUse \"symbrowse auth [command] --help\" for more information about a command.\n".to_owned(),
        ),
        ("auth", Some("login")) => Some(plain(
            "Resolve a vault entry and type the credentials into the detected login form",
            "symbrowse auth login <vault-entry> [flags]",
            "  -h, --help         help for login\n      --url string   navigate to this URL before detecting the login form\n",
            "Global Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --session string   session name (default \"default\")\n",
        )),
        ("completion", None) => Some(completion_help().to_owned()),
        ("compat-sidecar", None) => Some(plain(
            "Run the pinned Go/AzureTLS compatibility sidecar",
            "symbrowse compat-sidecar [flags]",
            "  -h, --help   help for compat-sidecar\n",
            global,
        )),
        ("doctor", None) => Some(plain(
            "Check browser discovery and local runtime prerequisites",
            "symbrowse doctor [flags]",
            "      --fix    print non-mutating, copyable remediation guidance\n  -h, --help   help for doctor\n",
            global,
        )),
        ("console", None) => Some(
            r#"Show or clear the page console buffer

Usage:
  symbrowse console [command]

Available Commands:
  clear       Clear the console buffer
  list        List captured console messages

Flags:
  -h, --help             help for console
      --session string   session name (default "default")

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")

Use "symbrowse console [command] --help" for more information about a command.
"#.to_owned(),
        ),
        ("console", Some("list")) => Some(plain(
            "List captured console messages",
            "symbrowse console list [flags]",
            "  -h, --help             help for list\n      --max-tokens int   token budget for the payload; oversized output is truncated and stored in the cache (0 = no limit)\n",
            "Global Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --session string   session name (default \"default\")\n",
        )),
        ("console", Some("clear")) => Some(plain(
            "Clear the console buffer",
            "symbrowse console clear [flags]",
            "  -h, --help   help for clear\n",
            session_global,
        )),
        ("errors", None) => Some(
            r#"Show or clear uncaught page errors

Usage:
  symbrowse errors [command]

Available Commands:
  clear       Clear the error buffer
  list        List uncaught exceptions with stack traces

Flags:
  -h, --help             help for errors
      --session string   session name (default "default")

Global Flags:
      --json            print the unified machine-readable output envelope (shorthand for --output json)
      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default "text")

Use "symbrowse errors [command] --help" for more information about a command.
"#.to_owned(),
        ),
        ("errors", Some("list")) => Some(plain(
            "List uncaught exceptions with stack traces",
            "symbrowse errors list [flags]",
            "  -h, --help             help for list\n      --max-tokens int   token budget for the payload; oversized output is truncated and stored in the cache (0 = no limit)\n",
            "Global Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --session string   session name (default \"default\")\n",
        )),
        ("errors", Some("clear")) => Some(plain(
            "Clear the error buffer",
            "symbrowse errors clear [flags]",
            "  -h, --help   help for clear\n",
            session_global,
        )),
        ("cookies", None) => Some("Inspect and manage cookies of the current page origin\n\nUsage:\n  symbrowse cookies [command]\n\nAvailable Commands:\n  clear       Delete one cookie by name\n  list        List cookies visible to the current page\n  set         Set a cookie (or import cookies from a curl cookie jar with --curl)\n\nFlags:\n  -h, --help             help for cookies\n      --reveal string    show cookie values (default: masked); accepts a comma-separated allowlist of cookie names or \"all\"\n      --session string   session name (default \"default\")\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n\nUse \"symbrowse cookies [command] --help\" for more information about a command.\n".to_owned()),
        ("cookies", Some("list")) => Some(plain(
            "List cookies visible to the current page", "symbrowse cookies list [flags]",
            "  -h, --help   help for list\n",
            "Global Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --reveal string    show cookie values (default: masked); accepts a comma-separated allowlist of cookie names or \"all\"\n      --session string   session name (default \"default\")\n",
        )),
        ("cookies", Some("clear")) => Some(plain(
            "Delete one cookie by name", "symbrowse cookies clear <name> [flags]",
            "  -h, --help         help for clear\n      --url string   URL scope of the cookie (default: current page URL)\n",
            "Global Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --reveal string    show cookie values (default: masked); accepts a comma-separated allowlist of cookie names or \"all\"\n      --session string   session name (default \"default\")\n",
        )),
        ("cookies", Some("set")) => Some(plain(
            "Set a cookie (or import cookies from a curl cookie jar with --curl)",
            "symbrowse cookies set <name> <value> [flags]",
            "      --curl string     import cookies from a curl cookie jar (Netscape format) file\n      --domain string   cookie domain (default: derived from the page URL)\n  -h, --help            help for set\n      --http-only       mark the cookie as HTTP-only\n      --path string     cookie path (default \"/\")\n      --secure          mark the cookie as secure-only\n      --url string      URL scope for the cookie (default: current page URL)\n",
            "Global Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --reveal string    show cookie values (default: masked); accepts a comma-separated allowlist of cookie names or \"all\"\n      --session string   session name (default \"default\")\n",
        )),
        ("a11y", None) => Some(plain(
            "Run an axe-core accessibility audit on the current page",
            "symbrowse a11y [url] [flags]",
            "  -h, --help              help for a11y\n      --selector string   restrict the audit to a CSS selector\n      --session string    daemon session name (default \"default\")\n      --tags string       comma-separated WCAG tags (e.g. wcag2a,wcag2aa)\n",
            global,
        )),
        ("screenshot", None) => Some("Capture the page (viewport, --full page, or --selector element)\n\nUsage:\n  symbrowse screenshot [path] [flags]\n\nFlags:\n      --format string           image format: png or jpeg (default \"png\")\n      --full                    capture the whole page, not just the viewport\n  -h, --help                    help for screenshot\n      --quality int             jpeg quality 0-100 (jpeg only)\n      --screenshot-dir string   allow writing the screenshot into this directory (default: the cache out directory)\n      --selector string         capture the element matched by a CSS selector or @ref\n      --session string          session name (default \"default\")\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n".to_owned()),
        ("set", None) => Some(
            "Apply session-wide emulation settings (viewport, device, geo, offline, headers, media, user-agent)\n\nUsage:\n  symbrowse set [command]\n\nAvailable Commands:\n  device      Apply a named device profile (run `set device --list` for the data table)\n  geo         Override the geolocation\n  headers     Override per-request headers (Authorization/Cookie headers are rejected)\n  media       Emulate the prefers-color-scheme media feature\n  offline     Emulate offline (default: on)\n  user-agent  Override the user agent string\n  viewport    Override the viewport size and device scale factor\n\nFlags:\n  -h, --help             help for set\n      --session string   session name (default \"default\")\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n\nUse \"symbrowse set [command] --help\" for more information about a command.\n".to_owned(),
        ),
        ("set", Some("device")) => Some(plain(
            "Apply a named device profile (run `set device --list` for the data table)",
            "symbrowse set device <name> [flags]",
            "  -h, --help   help for device\n      --list   list available device names\n",
            session_global,
        )),
        ("set", Some("geo")) => Some(plain(
            "Override the geolocation",
            "symbrowse set geo <latitude> <longitude> [flags]",
            "  -h, --help   help for geo\n",
            session_global,
        )),
        ("set", Some("headers")) => Some(plain(
            "Override per-request headers (Authorization/Cookie headers are rejected)",
            "symbrowse set headers <json> [flags]",
            "  -h, --help   help for headers\n",
            session_global,
        )),
        ("set", Some("media")) => Some(plain(
            "Emulate the prefers-color-scheme media feature",
            "symbrowse set media <dark|light> [flags]",
            "  -h, --help   help for media\n",
            session_global,
        )),
        ("set", Some("user-agent")) => Some(plain(
            "Override the user agent string",
            "symbrowse set user-agent <value> [flags]",
            "  -h, --help   help for user-agent\n",
            session_global,
        )),
        ("set", Some("viewport")) => Some(plain(
            "Override the viewport size and device scale factor",
            "symbrowse set viewport <width> <height> [scale] [flags]",
            "  -h, --help   help for viewport\n",
            session_global,
        )),
        ("set", Some("offline")) => Some(
            "Emulate offline (default: on)\n\nUsage:\n  symbrowse set offline [on|off] [flags]\n\nFlags:\n  -h, --help   help for offline\n\nGlobal Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --session string   session name (default \"default\")\n".to_owned(),
        ),
        ("journal", None) => Some("Inspect the append-only action journal\n\nUsage:\n  symbrowse journal [command]\n\nAvailable Commands:\n  show        Show the full journal of a session\n  tail        Show the last journal entries of a session\n\nFlags:\n  -h, --help             help for journal\n      --session string   session name (default \"default\")\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n\nUse \"symbrowse journal [command] --help\" for more information about a command.\n".to_owned()),
        ("journal", Some("tail")) => Some("Show the last journal entries of a session\n\nUsage:\n  symbrowse journal tail [flags]\n\nFlags:\n  -h, --help        help for tail\n      --lines int   number of entries to show (default 10)\n\nGlobal Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --session string   session name (default \"default\")\n".to_owned()),
        ("journal", Some("show")) => Some("Show the full journal of a session\n\nUsage:\n  symbrowse journal show [flags]\n\nFlags:\n  -h, --help   help for show\n\nGlobal Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --session string   session name (default \"default\")\n".to_owned()),
        ("network", None) => Some("Inspect, mock and export page network activity\n\nUsage:\n  symbrowse network [command]\n\nAvailable Commands:\n  request     Show one captured request by id\n  requests    List captured requests (sensitive headers masked)\n\nFlags:\n  -h, --help             help for network\n      --session string   session name (default \"default\")\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n\nUse \"symbrowse network [command] --help\" for more information about a command.\n".to_owned()),
        ("network", Some("request")) => Some(plain(
            "Show one captured request by id",
            "symbrowse network request <id> [flags]",
            "  -h, --help   help for request\n",
            "Global Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --session string   session name (default \"default\")\n",
        )),
        ("downloads", None) => Some("Show download events (origin URL, size, checksum) or set the download directory\n\nUsage:\n  symbrowse downloads [flags]\n\nFlags:\n      --dir string       set the download directory first\n  -h, --help             help for downloads\n      --session string   session name (default \"default\")\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n".to_owned()),
        ("network", Some("requests")) => Some("List captured requests (sensitive headers masked)\n\nUsage:\n  symbrowse network requests [flags]\n\nFlags:\n      --filter string    only URLs containing this substring\n  -h, --help             help for requests\n      --max-tokens int   token budget for the payload; oversized output is truncated and stored in the cache (0 = no limit)\n      --method string    only this HTTP method\n      --status int       only this HTTP status code\n      --type string      only this resource type (document, xhr, script, ...)\n\nGlobal Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --session string   session name (default \"default\")\n".to_owned()),
        ("diff", None) => Some("Compare snapshots, screenshots and URLs\n\nUsage:\n  symbrowse diff [command]\n\nAvailable Commands:\n  snapshot    Diff the current snapshot against a baseline file or the previous snapshot\n  url         Open two URLs and diff their extracted content\n\nFlags:\n  -h, --help             help for diff\n      --session string   daemon session name (default \"default\")\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n\nUse \"symbrowse diff [command] --help\" for more information about a command.\n".to_owned()),
        ("diff", Some("snapshot")) => Some("Diff the current snapshot against a baseline file or the previous snapshot\n\nUsage:\n  symbrowse diff snapshot [flags]\n\nFlags:\n      --baseline string   baseline snapshot JSON file to compare against\n  -h, --help              help for snapshot\n\nGlobal Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --session string   daemon session name (default \"default\")\n".to_owned()),
        ("diff", Some("url")) => Some("Open two URLs and diff their extracted content\n\nUsage:\n  symbrowse diff url <url1> <url2> [flags]\n\nFlags:\n  -h, --help   help for url\n\nGlobal Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --session string   daemon session name (default \"default\")\n".to_owned()),
        ("trace", None) => Some("Export and replay repeatable action traces\n\nUsage:\n  symbrowse trace [command]\n\nAvailable Commands:\n  export      Convert the session journal into a repeatable trace file\n  replay      Replay a trace file step by step and report deviations\n\nFlags:\n  -h, --help             help for trace\n      --session string   session name (default \"default\")\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n\nUse \"symbrowse trace [command] --help\" for more information about a command.\n".to_owned()),
        ("trace", Some("export")) => Some("Convert the session journal into a repeatable trace file\n\nUsage:\n  symbrowse trace export [flags]\n\nFlags:\n  -h, --help         help for export\n      --out string   trace file to write (default \"trace.json\")\n\nGlobal Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --session string   session name (default \"default\")\n".to_owned()),
        ("trace", Some("replay")) => Some("Replay a trace file step by step and report deviations\n\nUsage:\n  symbrowse trace replay <file> [flags]\n\nFlags:\n  -h, --help   help for replay\n\nGlobal Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --session string   session name (default \"default\")\n".to_owned()),
        ("watch", None) => Some("Watch an agent session: stream the action journal live (read-only)\n\nUsage:\n  symbrowse watch [flags]\n\nFlags:\n  -h, --help             help for watch\n      --reason string    handoff reason (required with --take-over)\n      --session string   session name (default \"default\")\n      --take-over        switch into a regular handoff instead of watching\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n".to_owned()),
        ("policy", None) => Some("Inspect the local risk policy\n\nUsage:\n  symbrowse policy [command]\n\nAvailable Commands:\n  explain     Show the effective decision for a command against a URL\n\nFlags:\n  -h, --help             help for policy\n      --session string   session name (default \"default\")\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n\nUse \"symbrowse policy [command] --help\" for more information about a command.\n".to_owned()),
        ("policy", Some("explain")) => Some("Show the effective decision for a command against a URL\n\nUsage:\n  symbrowse policy explain <command> [flags]\n\nFlags:\n  -h, --help          help for explain\n      --mode string   policy mode: mcp or tty (default: daemon mode)\n      --url string    URL whose host the rule is evaluated against\n\nGlobal Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --session string   session name (default \"default\")\n".to_owned()),
        ("version", None) => Some(plain(
            "Print the symbrowse version",
            "symbrowse version [flags]",
            "  -h, --help   help for version\n",
            global,
        )),
        ("eval", None) => Some(plain(
            "Execute JavaScript in the active page",
            "symbrowse eval <expression> [flags]",
            "  -b, --base64           expression is base64-encoded\n  -h, --help             help for eval\n      --session string   session name (default \"default\")\n      --stdin            read the expression from stdin\n",
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
        ("cache", None) => Some(
            "Inspect the truncate-and-store output cache\n\nUsage:\n  symbrowse cache [command]\n\nAvailable Commands:\n  clear       Remove all cache entries\n  get         Print a cached output (optionally one line range)\n  list        List cache entries with size, age and expiry\n\nFlags:\n  -h, --help   help for cache\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n\nUse \"symbrowse cache [command] --help\" for more information about a command.\n".to_owned(),
        ),
        ("cache", Some("clear")) => Some(plain(
            "Remove all cache entries",
            "symbrowse cache clear [flags]",
            "  -h, --help   help for clear\n",
            global,
        )),
        ("cache", Some("get")) => Some(plain(
            "Print a cached output (optionally one line range)",
            "symbrowse cache get <id> [flags]",
            "  -h, --help           help for get\n      --range string   1-indexed inclusive line range a-b (e.g. 40-120)\n",
            global,
        )),
        ("cache", Some("list")) => Some(plain(
            "List cache entries with size, age and expiry",
            "symbrowse cache list [flags]",
            "  -h, --help   help for list\n",
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
        ("storage", None) => Some(
            "Inspect and manage per-origin web storage\n\nUsage:\n  symbrowse storage [command]\n\nAvailable Commands:\n  clear       Remove all web storage values of one kind for the current origin\n  get         Read web storage values for the current origin\n  set         Write one web storage value for the current origin\n\nFlags:\n  -h, --help             help for storage\n      --session string   session name (default \"default\")\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n\nUse \"symbrowse storage [command] --help\" for more information about a command.\n".to_owned(),
        ),
        ("storage", Some("get")) => Some(plain(
            "Read web storage values for the current origin",
            "symbrowse storage get <local|session> [key] [flags]",
            "  -h, --help   help for get\n",
            session_global,
        )),
        ("storage", Some("set")) => Some(plain(
            "Write one web storage value for the current origin",
            "symbrowse storage set <local|session> <key> <value> [flags]",
            "  -h, --help   help for set\n",
            session_global,
        )),
        ("storage", Some("clear")) => Some(plain(
            "Remove all web storage values of one kind for the current origin",
            "symbrowse storage clear <local|session> [flags]",
            "  -h, --help   help for clear\n",
            session_global,
        )),
        ("upload", None) => Some("upload sets the value of a file input element matching <selector>.\n\nAccepted selector forms:\n  - CSS selector (e.g. \"button.submit\", \"#username\", \"input[name='q']\")\n  - Stable @eN ref from snapshot (e.g. \"@e1\", \"@e2\")\n  - Role/name pair as supported by the engine (e.g. role and accessible name)\n\nPositional arguments:\n  <selector>  Target file input element\n  <files...>  One or more local file paths to upload\n\nOptional [value] argument:\n  Not used; specify file paths as positional arguments.\n\nUsage:\n  symbrowse upload <selector> <files...> [flags]\n\nFlags:\n  -h, --help             help for upload\n      --session string   session name (default \"default\")\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n".to_owned()),
        ("session", None) => Some(
            "Inspect browser sessions\n\nUsage:\n  symbrowse session [command]\n\nAvailable Commands:\n  id          Derive a stable, collision-free session id from the local repository layout\n  info        Show session information\n  list        List sessions\n\nFlags:\n  -h, --help             help for session\n      --session string   session name (default \"default\")\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n\nUse \"symbrowse session [command] --help\" for more information about a command.\n".to_owned(),
        ),
        ("session", Some("id")) => Some(plain(
            "Derive a stable, collision-free session id from the local repository layout",
            "symbrowse session id [flags]",
            "  -h, --help            help for id\n      --prefix string   optional id prefix (e.g. an agent name)\n      --scope string    anchor scope: worktree, repo or cwd (default \"worktree\")\n",
            session_global,
        )),
        ("session", Some("list")) => Some(plain(
            "List sessions",
            "symbrowse session list [flags]",
            "  -h, --help   help for list\n",
            session_global,
        )),
        ("session", Some("info")) => Some(plain(
            "Show session information",
            "symbrowse session info [flags]",
            "  -h, --help   help for info\n",
            session_global,
        )),
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

fn interaction_help(action: &str) -> String {
    let value_help = match action {
        "select" => "The value or label of the option to select from the drop-down.",
        "scroll" => "The vertical scroll amount in pixels (positive for down, negative for up).",
        _ => "Not used for this interaction.",
    };
    format!(
        "{action} performs the {action} interaction on the targeted element.\n\nAccepted selector forms:\n  - CSS selector (e.g. \"button.submit\", \"#username\", \"input[name='q']\")\n  - Stable @eN ref from snapshot (e.g. \"@e1\", \"@e2\")\n  - Role/name pair as supported by the engine (e.g. role and accessible name)\n\nOptional [value] argument:\n  {value_help}\n\nUsage:\n  symbrowse {action} <selector> [value] [flags]\n\nFlags:\n  -h, --help             help for {action}\n      --session string   session name (default \"default\")\n\nGlobal Flags:\n      --json            print the unified machine-readable output envelope (shorthand for --output json)\n      --output string   output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n"
    )
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
    let interaction = matches!(
        name,
        "check"
            | "dblclick"
            | "focus"
            | "hover"
            | "select"
            | "uncheck"
            | "scroll"
            | "scrollintoview"
    );
    let mut session = "default".to_owned();
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut positional = Vec::new();
    let mut args = serde_json::Map::new();
    let mut index = command_index + 1;
    let mut positional_only = false;
    while index < values.len() {
        let value = &values[index];
        if name == "upload"
            && !positional_only
            && value.starts_with('-')
            && !matches!(value.as_str(), "--" | "--json" | "--output" | "--session")
            && !value.starts_with("--json=")
            && !value.starts_with("--output=")
            && !value.starts_with("--session=")
        {
            return Err(unknown_flag(value));
        }
        if interaction
            && !positional_only
            && value.starts_with('-')
            && !matches!(value.as_str(), "--" | "--json" | "--output" | "--session")
            && !value.starts_with("--json=")
            && !value.starts_with("--output=")
            && !value.starts_with("--session=")
        {
            return Err(unknown_flag(value));
        }
        if positional_only {
            positional.push(value.clone());
            index += 1;
            continue;
        }
        match value.as_str() {
            "--" => positional_only = true,
            "--json" => json = true,
            _ if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
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
            "--tags" if name == "a11y" => {
                index += 1;
                let tags = required_value(values, index, "--tags")?
                    .split(',')
                    .map(str::trim)
                    .filter(|tag| !tag.is_empty())
                    .map(|tag| serde_json::Value::String(tag.to_owned()))
                    .collect();
                args.insert("tags".into(), serde_json::Value::Array(tags));
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
            "--quality" if name == "screenshot" => {
                index += 1;
                let quality = required_value(values, index, "--quality")?
                    .parse::<i64>()
                    .map_err(|_| ParseError {
                        message: "invalid quality".into(),
                        exit_code: 2,
                    })?;
                args.insert("quality".into(), serde_json::Value::from(quality));
            }
            "--screenshot-dir" if name == "screenshot" => {
                index += 1;
                args.insert(
                    "dir".into(),
                    serde_json::Value::String(
                        required_value(values, index, "--screenshot-dir")?.into(),
                    ),
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
            "--full" if name == "screenshot" => {
                args.insert("full".into(), serde_json::Value::Bool(true));
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
            value if name == "a11y" && value.starts_with("--tags=") => {
                let tags = value[7..]
                    .split(',')
                    .map(str::trim)
                    .filter(|tag| !tag.is_empty())
                    .map(|tag| serde_json::Value::String(tag.to_owned()))
                    .collect();
                args.insert("tags".into(), serde_json::Value::Array(tags));
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
            value if name == "screenshot" && value.starts_with("--quality=") => {
                let quality = value[10..].parse::<i64>().map_err(|_| ParseError {
                    message: "invalid quality".into(),
                    exit_code: 2,
                })?;
                args.insert("quality".into(), serde_json::Value::from(quality));
            }
            value if name == "screenshot" && value.starts_with("--screenshot-dir=") => {
                args.insert("dir".into(), serde_json::Value::String(value[17..].into()));
            }
            value if value.starts_with('-') => {
                return Err(unknown_flag(value));
            }
            _ => positional.push(value.clone()),
        }
        index += 1;
    }
    let supplied_positional_count = positional.len();
    match name {
        "upload" => {
            if supplied_positional_count < 2 {
                return Err(ParseError {
                    message: format!(
                        "requires at least 2 arg(s), only received {supplied_positional_count}"
                    ),
                    exit_code: 2,
                });
            }
            args.insert(
                "selector".into(),
                serde_json::Value::String(positional.remove(0)),
            );
            args.insert(
                "files".into(),
                serde_json::Value::Array(
                    positional
                        .drain(..)
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
        }
        "fetch" | "read" | "open" | "goto" => {
            take_positional(&mut args, &mut positional, "url");
        }
        "click" | "fill" | "check" | "dblclick" | "focus" | "hover" | "select" | "uncheck"
        | "scroll" | "scrollintoview" => {
            take_positional(&mut args, &mut positional, "selector");
        }
        "press" => take_positional(&mut args, &mut positional, "key"),
        "get" | "is" => take_positional(&mut args, &mut positional, "kind"),
        "a11y" => {
            take_positional(&mut args, &mut positional, "url");
            args.entry("tags").or_insert(serde_json::Value::Null);
            args.entry("selector")
                .or_insert_with(|| serde_json::Value::String(String::new()));
        }
        "screenshot" => {
            take_positional(&mut args, &mut positional, "path");
            args.entry("full").or_insert(serde_json::Value::Bool(false));
            args.entry("selector")
                .or_insert_with(|| serde_json::Value::String(String::new()));
            args.entry("format")
                .or_insert_with(|| serde_json::Value::String("png".into()));
            args.entry("quality").or_insert(serde_json::Value::from(0));
            args.entry("dir")
                .or_insert_with(|| serde_json::Value::String(String::new()));
        }
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
    if interaction {
        if name == "select" {
            take_positional(&mut args, &mut positional, "value");
        }
        if name == "scroll" {
            if let Some(amount) = positional.first() {
                let amount = amount.parse::<i64>().map_err(|_| ParseError {
                    message: format!(
                        "scroll amount: strconv.ParseInt: parsing {amount:?}: invalid syntax"
                    ),
                    exit_code: 2,
                })?;
                args.insert("amount".into(), serde_json::Value::from(amount));
                positional.remove(0);
            }
        }
        let max = if name == "select" || name == "scroll" {
            2
        } else {
            1
        };
        if supplied_positional_count == 0 || supplied_positional_count > max {
            return Err(ParseError {
                message: format!("{name} requires a selector and optional value"),
                exit_code: 2,
            });
        }
        args.insert("action".into(), serde_json::Value::String(name.to_owned()));
    }
    if json {
        format = Format::Json;
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
        if matches!(name, "fetch" | "goto" | "open" | "a11y" | "screenshot") {
            return Err(ParseError {
                message: format!("accepts at most 1 arg(s), received {supplied_positional_count}"),
                exit_code: 2,
            });
        }
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
        "check" | "dblclick" | "focus" | "hover" | "select" | "uncheck" | "scroll"
        | "scrollintoview" => &["selector"][..],
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

fn parse_storage(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut session = String::from("default");
    let mut subcommand = None;
    let mut positional = Vec::new();
    let mut positional_only = false;
    let mut index = command_index + 1;
    while index < values.len() {
        let value = &values[index];
        if positional_only {
            positional.push(value.clone());
            index += 1;
            continue;
        }
        match value.as_str() {
            "--" => positional_only = true,
            "get" | "set" | "clear" if subcommand.is_none() => subcommand = Some(value.as_str()),
            "--json" => json = true,
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            _ if subcommand.is_none() => {
                return Err(ParseError {
                    message: format!("unknown command {value:?} for \"symbrowse storage\""),
                    exit_code: 2,
                });
            }
            _ => positional.push(value.clone()),
        }
        index += 1;
    }
    if json {
        format = Format::Json;
    }
    let Some(subcommand) = subcommand else {
        return Ok(Action::Help(command_help("storage", &[]).unwrap()));
    };
    match subcommand {
        "get" if (1..=2).contains(&positional.len()) => Ok(Action::StorageGet {
            session,
            kind: positional[0].clone(),
            key: positional.get(1).cloned(),
            format,
        }),
        "get" => Err(ParseError {
            message: format!(
                "accepts between 1 and 2 arg(s), received {}",
                positional.len()
            ),
            exit_code: 2,
        }),
        "set" if positional.len() == 3 => Ok(Action::Dispatch {
            session,
            command: "storage.set".to_owned(),
            args: serde_json::json!({
                "kind": positional[0], "key": positional[1], "value": positional[2]
            }),
            format,
        }),
        "set" => Err(ParseError {
            message: format!("accepts 3 arg(s), received {}", positional.len()),
            exit_code: 2,
        }),
        "clear" if positional.len() == 1 => Ok(Action::Dispatch {
            session,
            command: "storage.clear".to_owned(),
            args: serde_json::json!({"kind": positional[0]}),
            format,
        }),
        "clear" => Err(ParseError {
            message: format!("accepts 1 arg(s), received {}", positional.len()),
            exit_code: 2,
        }),
        _ => unreachable!("storage subcommand selected from supported names"),
    }
}

fn render_journal_text(data: &serde_json::Value) -> String {
    data.get("entries")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|entry| {
            let field = |name| {
                entry
                    .get(name)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
            };
            format!(
                "{}\t{}\t{}\t{}\t{}\n",
                field("timestamp"),
                field("command"),
                field("risk_class"),
                field("decider"),
                field("result")
            )
        })
        .collect()
}

fn parse_handoff(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut session = String::from("default");
    let mut reason = String::new();
    let mut timeout = String::from("5m");
    let mut index = command_index + 1;
    while index < values.len() {
        let value = &values[index];
        match value.as_str() {
            "--reason" | "--timeout" | "--session" | "--output" => {
                index += 1;
                let supplied = required_value(values, index, value)?;
                match value.as_str() {
                    "--reason" => reason = supplied.to_owned(),
                    "--timeout" => timeout = supplied.to_owned(),
                    "--session" => session = supplied.to_owned(),
                    _ => format = parse_format(supplied)?,
                }
            }
            "--json" => json = true,
            value if value.starts_with("--reason=") => reason = value[9..].to_owned(),
            value if value.starts_with("--timeout=") => timeout = value[10..].to_owned(),
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            _ => {
                return Err(ParseError {
                    message: "accepts 0 arg(s), received 1".into(),
                    exit_code: 2,
                });
            }
        }
        index += 1;
    }
    if reason.is_empty() {
        return Err(ParseError {
            message: "handoff requires --reason".into(),
            exit_code: 2,
        });
    }
    if json {
        format = Format::Json;
    }
    Ok(Action::Dispatch {
        session,
        command: "handoff".into(),
        args: serde_json::json!({"reason": reason, "timeout": timeout}),
        format,
    })
}

fn parse_oob(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut session = String::from("default");
    let mut status = false;
    let mut index = command_index + 1;
    while index < values.len() {
        let value = &values[index];
        match value.as_str() {
            "status" if !status => status = true,
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            "--json" => json = true,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            _ => {
                return Err(ParseError {
                    message: format!("unknown command {value:?} for \"symbrowse oob\""),
                    exit_code: 2,
                });
            }
        }
        index += 1;
    }
    if !status {
        return Ok(Action::Help(
            help_catalog::help("oob", &[])
                .unwrap_or_default()
                .to_owned(),
        ));
    }
    if json {
        format = Format::Json;
    }
    Ok(Action::Dispatch {
        session,
        command: "oob.status".into(),
        args: serde_json::json!({}),
        format,
    })
}

fn parse_auth(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut session = String::from("default");
    let mut entry = None;
    let mut url = None;
    let mut login_seen = false;
    let mut index = command_index + 1;
    while index < values.len() {
        let value = &values[index];
        match value.as_str() {
            "login" if !login_seen => login_seen = true,
            "--json" => json = true,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            "--url" if login_seen => {
                index += 1;
                url = Some(required_value(values, index, "--url")?.to_owned());
            }
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with("--url=") && login_seen => url = Some(value[6..].to_owned()),
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            value if !login_seen => {
                return Err(ParseError {
                    message: format!("unknown command {value:?} for \"symbrowse auth\""),
                    exit_code: 2,
                });
            }
            value if entry.is_none() => entry = Some(value.to_owned()),
            _ => {
                return Err(ParseError {
                    message: format!(
                        "accepts 1 arg(s), received {}",
                        values[command_index + 2..]
                            .iter()
                            .filter(|value| !value.starts_with('-'))
                            .count()
                    ),
                    exit_code: 2,
                });
            }
        }
        index += 1;
    }
    if !login_seen {
        return Ok(Action::Help(
            command_help("auth", &[]).expect("auth help is defined"),
        ));
    }
    let Some(entry) = entry else {
        return Err(ParseError {
            message: "accepts 1 arg(s), received 0".into(),
            exit_code: 2,
        });
    };
    if json {
        format = Format::Json;
    }
    let mut args = serde_json::Map::new();
    args.insert("entry".into(), serde_json::Value::String(entry));
    if let Some(url) = url {
        args.insert("url".into(), serde_json::Value::String(url));
    }
    Ok(Action::Dispatch {
        session,
        command: "auth.login".into(),
        args: serde_json::Value::Object(args),
        format,
    })
}

fn parse_journal(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut session = String::from("default");
    let mut lines = 10i64;
    let mut subcommand = None;
    let mut index = command_index + 1;
    while index < values.len() {
        let value = &values[index];
        match value.as_str() {
            "tail" | "show" if subcommand.is_none() => subcommand = Some(value.as_str()),
            "--json" => json = true,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            "--lines" if subcommand == Some("tail") => {
                index += 1;
                lines = required_value(values, index, "--lines")?.parse::<i64>().map_err(|_| ParseError {
                    message: format!("invalid argument \"{}\" for \"--lines\" flag: strconv.ParseInt: parsing \"{}\": invalid syntax", values[index], values[index]),
                    exit_code: 2,
                })?;
            }
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with("--lines=") => lines = value[8..].parse::<i64>().map_err(|_| ParseError {
                message: format!("invalid argument \"{}\" for \"--lines\" flag: strconv.ParseInt: parsing \"{}\": invalid syntax", &value[8..], &value[8..]),
                exit_code: 2,
            })?,
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            _ if subcommand.is_none() => {
                return Ok(Action::Help(command_help("journal", &[]).unwrap()));
            }
            _ => return Err(ParseError {
                message: format!("unknown command {value:?} for \"symbrowse journal {}\"", subcommand.unwrap()),
                exit_code: 2,
            }),
        }
        index += 1;
    }
    if json {
        format = Format::Json;
    }
    match subcommand {
        None => Ok(Action::Help(command_help("journal", &[]).unwrap())),
        Some("show") => Ok(Action::Dispatch {
            session: session.clone(),
            command: "journal.show".into(),
            args: serde_json::json!({"session": session}),
            format,
        }),
        Some("tail") => Ok(Action::Dispatch {
            session: session.clone(),
            command: "journal.tail".into(),
            args: serde_json::json!({"session": session, "lines": lines}),
            format,
        }),
        _ => unreachable!("journal subcommand selected from supported names"),
    }
}

fn parse_watch(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut session = String::from("default");
    let mut take_over = false;
    let mut reason = String::new();
    let mut index = command_index + 1;
    while index < values.len() {
        let value = &values[index];
        match value.as_str() {
            "--json" => json = true,
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            "--take-over" => take_over = true,
            value if value.starts_with("--take-over=") => {
                take_over = parse_bool("--take-over", &value[12..])?;
            }
            "--reason" => {
                index += 1;
                reason = required_value(values, index, "--reason")?.to_owned();
            }
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with("--json=") => {
                json = parse_bool("--json", &value[7..])?;
            }
            value if value.starts_with("--reason=") => {
                reason = value[9..].to_owned();
            }
            value if value.starts_with("--output=") => {
                format = parse_format(&value[9..])?;
            }
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            _ => {
                return Err(ParseError {
                    message: format!("unknown argument {value:?}"),
                    exit_code: 2,
                });
            }
        }
        index += 1;
    }
    if take_over && reason.is_empty() {
        return Err(ParseError {
            message: "watch --take-over requires --reason".into(),
            exit_code: 2,
        });
    }
    if json {
        format = Format::Json;
    }
    Ok(Action::Watch {
        session,
        take_over,
        reason,
        format,
    })
}

fn parse_trace(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut session = String::from("default");
    let mut path = PathBuf::from("trace.json");
    let mut subcommand = None;
    let mut positional = Vec::new();
    let mut index = command_index + 1;
    while index < values.len() {
        let value = &values[index];
        match value.as_str() {
            "export" if subcommand.is_none() => subcommand = Some("export"),
            "replay" if subcommand.is_none() => subcommand = Some("replay"),
            "--json" => json = true,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            "--out" if subcommand == Some("export") => {
                index += 1;
                path = PathBuf::from(required_value(values, index, "--out")?);
            }
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with("--out=") && subcommand == Some("export") => {
                path = PathBuf::from(&value[6..]);
            }
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            value if subcommand.is_none() => {
                return Err(ParseError {
                    message: format!("unknown command {value:?} for \"symbrowse trace\""),
                    exit_code: 2,
                });
            }
            value => positional.push(value.to_owned()),
        }
        index += 1;
    }
    if json {
        format = Format::Json;
    }
    match subcommand {
        None => Ok(Action::Help(command_help("trace", &[]).unwrap())),
        Some("export") if positional.is_empty() => Ok(Action::TraceExport {
            session,
            path,
            format,
        }),
        Some("export") => Err(ParseError {
            message: format!(
                "unknown command {:?} for \"symbrowse trace export\"",
                positional[0]
            ),
            exit_code: 2,
        }),
        Some("replay") if positional.len() == 1 => Ok(Action::TraceReplay {
            session,
            path: PathBuf::from(&positional[0]),
            format,
        }),
        Some("replay") => Err(ParseError {
            message: format!("accepts 1 arg(s), received {}", positional.len()),
            exit_code: 2,
        }),
        _ => unreachable!("trace subcommand selected from supported names"),
    }
}

fn parse_diff(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut session = String::from("default");
    let mut baseline = None;
    let mut subcommand = None;
    let mut positional = Vec::new();
    let mut index = command_index + 1;
    while index < values.len() {
        let value = &values[index];
        match value.as_str() {
            "snapshot" if subcommand.is_none() => subcommand = Some("snapshot"),
            "url" if subcommand.is_none() => subcommand = Some("url"),
            "--json" => json = true,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            "--baseline" if subcommand == Some("snapshot") => {
                index += 1;
                baseline = Some(PathBuf::from(required_value(values, index, "--baseline")?));
            }
            value if value.starts_with("--json=") => {
                json = parse_bool("--json", &value[7..])?;
            }
            value if value.starts_with("--output=") => {
                format = parse_format(&value[9..])?;
            }
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with("--baseline=") && subcommand == Some("snapshot") => {
                baseline = Some(PathBuf::from(&value[11..]));
            }
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            value if subcommand.is_none() => {
                return Err(ParseError {
                    message: format!("unknown command {value:?} for \"symbrowse diff\""),
                    exit_code: 2,
                });
            }
            value => positional.push(value.to_owned()),
        }
        index += 1;
    }
    if json {
        format = Format::Json;
    }
    match subcommand {
        None => Ok(Action::Help(command_help("diff", &[]).unwrap())),
        Some("snapshot") if positional.is_empty() => Ok(Action::DiffSnapshot {
            session,
            baseline,
            format,
        }),
        Some("snapshot") => Err(ParseError {
            message: format!(
                "unknown command {:?} for \"symbrowse diff snapshot\"",
                positional[0]
            ),
            exit_code: 2,
        }),
        Some("url") if positional.len() == 2 => Ok(Action::DiffUrl {
            session,
            first_url: positional[0].clone(),
            second_url: positional[1].clone(),
            format,
        }),
        Some("url") => Err(ParseError {
            message: format!("requires 2 arg(s), only received {}", positional.len()),
            exit_code: 2,
        }),
        _ => unreachable!("diff subcommand selected from supported names"),
    }
}

fn parse_network(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut session = String::from("default");
    let mut subcommand = None;
    let mut args = serde_json::Map::new();
    let mut positional = Vec::new();
    let mut index = command_index + 1;
    while index < values.len() {
        let value = &values[index];
        match value.as_str() {
            "requests" if subcommand.is_none() => subcommand = Some("requests"),
            "request" if subcommand.is_none() => subcommand = Some("request"),
            "--json" => json = true,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            "--filter" | "--type" | "--method" | "--status" | "--max-tokens" => {
                let name = value.as_str();
                index += 1;
                let argument = required_value(values, index, name)?;
                if name == "--status" || name == "--max-tokens" {
                    let number = argument.parse::<i64>().map_err(|_| ParseError {
                        message: format!("invalid value {argument:?} for {name}"),
                        exit_code: 2,
                    })?;
                    args.insert(name[2..].replace('-', "_").into(), number.into());
                } else {
                    args.insert(name[2..].into(), argument.into());
                }
            }
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value
                if value.starts_with("--filter=")
                    || value.starts_with("--type=")
                    || value.starts_with("--method=")
                    || value.starts_with("--status=")
                    || value.starts_with("--max-tokens=") =>
            {
                let (name, argument) = value.split_once('=').expect("equals flag");
                if name == "--status" || name == "--max-tokens" {
                    let number = argument.parse::<i64>().map_err(|_| ParseError {
                        message: format!("invalid value {argument:?} for {name}"),
                        exit_code: 2,
                    })?;
                    args.insert(name[2..].replace('-', "_").into(), number.into());
                } else {
                    args.insert(name[2..].into(), argument.into());
                }
            }
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            _ if subcommand.is_none() => {
                return Err(ParseError {
                    message: format!("unknown command {value:?} for \"symbrowse network\""),
                    exit_code: 2,
                });
            }
            _ => positional.push(value.clone()),
        }
        index += 1;
    }
    if json {
        format = Format::Json;
    }
    match subcommand {
        None => Ok(Action::Help(command_help("network", &[]).unwrap())),
        Some("requests") if positional.is_empty() => Ok(Action::Dispatch {
            session,
            command: "network.requests".into(),
            args: serde_json::Value::Object(args),
            format,
        }),
        Some("requests") => Err(ParseError {
            message: format!(
                "unknown command {:?} for \"symbrowse network requests\"",
                positional[0]
            ),
            exit_code: 2,
        }),
        Some("request") if positional.len() == 1 => Ok(Action::Dispatch {
            session,
            command: "network.request".into(),
            args: serde_json::json!({"id": positional[0]}),
            format,
        }),
        Some("request") if positional.is_empty() => Err(ParseError {
            message: "accepts 1 arg(s), received 0".to_owned(),
            exit_code: 2,
        }),
        Some("request") => Err(ParseError {
            message: format!("accepts 1 arg(s), received {}", positional.len()),
            exit_code: 2,
        }),
        _ => unreachable!("network subcommand selected from supported names"),
    }
}

fn parse_policy(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut session = String::from("default");
    let mut url = String::new();
    let mut mode = String::new();
    let mut subcommand = None;
    let mut positional = Vec::new();
    let mut index = command_index + 1;
    while index < values.len() {
        let value = &values[index];
        match value.as_str() {
            "explain" if subcommand.is_none() => subcommand = Some("explain"),
            "--json" => json = true,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            "--url" => {
                index += 1;
                url = required_value(values, index, "--url")?.to_owned();
            }
            "--mode" => {
                index += 1;
                mode = required_value(values, index, "--mode")?.to_owned();
            }
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with("--url=") => url = value[6..].to_owned(),
            value if value.starts_with("--mode=") => mode = value[7..].to_owned(),
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            _ if subcommand.is_none() => {
                return Err(ParseError {
                    message: format!("unknown command {value:?} for \"symbrowse policy\""),
                    exit_code: 2,
                });
            }
            _ => positional.push(value.clone()),
        }
        index += 1;
    }
    if json {
        format = Format::Json;
    }
    match subcommand {
        None => Ok(Action::Help(command_help("policy", &[]).unwrap())),
        Some("explain") if positional.len() == 1 => Ok(Action::Dispatch {
            session,
            command: "policy.explain".into(),
            args: serde_json::json!({
                "command": positional[0], "url": url, "mode": mode
            }),
            format,
        }),
        Some("explain") => Err(ParseError {
            message: format!("accepts 1 arg(s), received {}", positional.len()),
            exit_code: 2,
        }),
        _ => unreachable!("policy subcommand selected from supported names"),
    }
}

fn parse_cookies(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut session = String::from("default");
    let mut reveal = String::new();
    let mut url = String::new();
    let mut domain = String::new();
    let mut path = String::from("/");
    let mut secure = false;
    let mut http_only = false;
    let mut curl_path: Option<PathBuf> = None;
    let mut subcommand = None;
    let mut positional = Vec::new();
    let mut positional_only = false;
    let mut index = command_index + 1;
    while index < values.len() {
        let value = &values[index];
        if positional_only {
            positional.push(value.clone());
            index += 1;
            continue;
        }
        match value.as_str() {
            "--" => positional_only = true,
            "list" | "clear" | "set" if subcommand.is_none() => subcommand = Some(value.as_str()),
            "--json" => json = true,
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            "--reveal" => {
                index += 1;
                reveal = required_value(values, index, "--reveal")?.to_owned();
            }
            value if value.starts_with("--reveal=") => reveal = value[9..].to_owned(),
            "--url" => {
                index += 1;
                url = required_value(values, index, "--url")?.to_owned();
            }
            value if value.starts_with("--url=") => url = value[6..].to_owned(),
            "--domain" => {
                index += 1;
                domain = required_value(values, index, "--domain")?.to_owned();
            }
            value if value.starts_with("--domain=") => domain = value[9..].to_owned(),
            "--path" => {
                index += 1;
                path = required_value(values, index, "--path")?.to_owned();
            }
            value if value.starts_with("--path=") => path = value[7..].to_owned(),
            "--secure" => secure = true,
            value if value.starts_with("--secure=") => {
                secure = parse_bool("--secure", &value[9..])?
            }
            "--http-only" => http_only = true,
            value if value.starts_with("--http-only=") => {
                http_only = parse_bool("--http-only", &value[12..])?;
            }
            "--curl" => {
                index += 1;
                curl_path = Some(PathBuf::from(required_value(values, index, "--curl")?));
            }
            value if value.starts_with("--curl=") => curl_path = Some(PathBuf::from(&value[7..])),
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            _ if subcommand.is_none() => {
                return Err(ParseError {
                    message: format!("unknown command {value:?} for \"symbrowse cookies\""),
                    exit_code: 2,
                });
            }
            _ => positional.push(value.clone()),
        }
        index += 1;
    }
    if json {
        format = Format::Json;
    }
    let Some(subcommand) = subcommand else {
        return Ok(Action::Help(command_help("cookies", &[]).unwrap()));
    };
    match subcommand {
        "list" if positional.is_empty() && url.is_empty() => Ok(Action::Dispatch {
            session,
            command: "cookies.list".into(),
            args: serde_json::json!({"reveal": reveal}),
            format,
        }),
        "list" => Err(if let Some(extra) = positional.first() {
            ParseError {
                message: format!("unknown command {extra:?} for \"symbrowse cookies list\""),
                exit_code: 2,
            }
        } else {
            unknown_flag("--url")
        }),
        "clear" if positional.len() == 1 => Ok(Action::Dispatch {
            session,
            command: "cookies.clear".into(),
            args: serde_json::json!({"name": positional[0], "url": url}),
            format,
        }),
        "clear" => Err(ParseError {
            message: format!("accepts 1 arg(s), received {}", positional.len()),
            exit_code: 2,
        }),
        "set" if curl_path.is_some() && positional.is_empty() => Ok(Action::CookieImport {
            session,
            path: curl_path.unwrap(),
        }),
        "set" if curl_path.is_some() => Err(ParseError {
            message: format!("accepts 0 arg(s), received {}", positional.len()),
            exit_code: 2,
        }),
        "set" if positional.len() == 2 => Ok(Action::Dispatch {
            session,
            command: "cookies.set".into(),
            args: serde_json::json!({
                "cookie": {
                    "name": positional[0], "value": positional[1], "domain": domain,
                    "path": path, "expires": 0.0, "size": 0, "secure": secure,
                    "http_only": http_only, "session": false
                },
                "url": url
            }),
            format,
        }),
        "set" => Err(ParseError {
            message: format!("accepts 2 arg(s), received {}", positional.len()),
            exit_code: 2,
        }),
        _ => unreachable!("cookies subcommand selected from supported names"),
    }
}

fn parse_session(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut session = String::from("default");
    let mut scope = String::from("worktree");
    let mut prefix = String::new();
    let mut subcommand = None;
    let mut positional = Vec::new();
    let mut positional_only = false;
    let mut index = command_index + 1;
    while index < values.len() {
        let value = &values[index];
        if positional_only {
            positional.push(value.clone());
            index += 1;
            continue;
        }
        match value.as_str() {
            "--" => positional_only = true,
            "list" | "info" | "id" if subcommand.is_none() => subcommand = Some(value.as_str()),
            "--json" => json = true,
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            "--scope" => {
                index += 1;
                scope = required_value(values, index, "--scope")?.to_owned();
            }
            value if value.starts_with("--scope=") => scope = value[8..].to_owned(),
            "--prefix" => {
                index += 1;
                prefix = required_value(values, index, "--prefix")?.to_owned();
            }
            value if value.starts_with("--prefix=") => prefix = value[9..].to_owned(),
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            _ if subcommand.is_none() => {
                return Err(ParseError {
                    message: format!("unknown command {value:?} for \"symbrowse session\""),
                    exit_code: 2,
                });
            }
            _ => positional.push(value.clone()),
        }
        index += 1;
    }
    if json {
        format = Format::Json;
    }
    let Some(subcommand) = subcommand else {
        return Ok(Action::Help(command_help("session", &[]).unwrap()));
    };
    if !positional.is_empty() {
        return Err(ParseError {
            message: format!("unknown argument {:?}", positional[0]),
            exit_code: 2,
        });
    }
    if subcommand == "id" {
        return Ok(Action::SessionId {
            scope,
            prefix,
            format,
        });
    }
    Ok(Action::Dispatch {
        session,
        command: format!("session.{subcommand}"),
        args: serde_json::json!({}),
        format,
    })
}

fn parse_cache(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut range = None;
    let mut subcommand = None;
    let mut positional = Vec::new();
    let mut positional_only = false;
    let mut index = command_index + 1;
    while index < values.len() {
        let value = &values[index];
        if positional_only {
            positional.push(value.clone());
            index += 1;
            continue;
        }
        match value.as_str() {
            "--" => positional_only = true,
            "get" if subcommand.is_none() => subcommand = Some("get"),
            "clear" if subcommand.is_none() => subcommand = Some("clear"),
            "list" if subcommand.is_none() => subcommand = Some("list"),
            "--json" => json = true,
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            "--range" => {
                index += 1;
                range = Some(required_value(values, index, "--range")?.to_owned());
            }
            value if value.starts_with("--range=") => range = Some(value[8..].to_owned()),
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            _ if subcommand.is_none() => {
                return Err(ParseError {
                    message: format!("unknown command {value:?} for \"symbrowse cache\""),
                    exit_code: 2,
                });
            }
            _ => positional.push(value.clone()),
        }
        index += 1;
    }
    if json {
        format = Format::Json;
    }
    match subcommand {
        None => Ok(Action::Help(
            command_help("cache", &[]).expect("cache help is defined"),
        )),
        Some("clear") if positional.is_empty() && range.is_none() => {
            Ok(Action::CacheClear { format })
        }
        Some("clear") => Err(ParseError {
            message: format!(
                "unknown command {:?} for \"symbrowse cache clear\"",
                positional.first().map(String::as_str).unwrap_or("--range")
            ),
            exit_code: 2,
        }),
        Some("list") if positional.is_empty() && range.is_none() => {
            Ok(Action::CacheList { format })
        }
        Some("list") => Err(ParseError {
            message: format!(
                "unknown command {:?} for \"symbrowse cache list\"",
                positional.first().map(String::as_str).unwrap_or("--range")
            ),
            exit_code: 2,
        }),
        Some("get") if positional.len() == 1 => Ok(Action::CacheGet {
            id: positional.remove(0),
            range,
            format,
        }),
        Some("get") => Err(ParseError {
            message: format!("accepts 1 arg(s), received {}", positional.len()),
            exit_code: 2,
        }),
        _ => unreachable!("cache subcommand selected from supported names"),
    }
}

fn parse_dialog(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut session = String::from("default");
    let mut subcommand = None;
    let mut subcommand_index = None;
    let mut scan = command_index + 1;
    while scan < values.len() {
        match values[scan].as_str() {
            "accept" | "auto" | "dismiss" | "status" => {
                subcommand = Some(values[scan].as_str());
                subcommand_index = Some(scan);
                break;
            }
            "--session" => {
                session = required_value(values, scan + 1, "--session")?.to_owned();
                scan += 1;
            }
            "--output" => {
                format = parse_format(required_value(values, scan + 1, "--output")?)?;
                scan += 1;
            }
            "--json" => {}
            value if value.starts_with("--json=") => {
                json = parse_bool("--json", &value[7..])?;
            }
            value if value.starts_with("--output=") => {
                format = parse_format(&value[9..])?;
            }
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            _ => {
                return Ok(Action::Help(
                    help_catalog::help("dialog", &[]).unwrap().to_owned(),
                ));
            }
        }
        scan += 1;
    }
    let Some(subcommand) = subcommand else {
        return Ok(Action::Help(
            help_catalog::help("dialog", &[]).unwrap().to_owned(),
        ));
    };
    let mut positional = Vec::new();
    let mut positional_only = false;
    let mut index = command_index + 1;
    while index < values.len() {
        if Some(index) == subcommand_index {
            index += 1;
            continue;
        }
        let value = &values[index];
        if positional_only {
            positional.push(value.clone());
            index += 1;
            continue;
        }
        match value.as_str() {
            "--" => positional_only = true,
            "--json" => json = true,
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            value if value.starts_with("--json=") => {
                json = parse_bool("--json", &value[7..])?;
            }
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            _ => positional.push(value.clone()),
        }
        index += 1;
    }
    if json {
        format = Format::Json;
    }
    let (command, args) = match subcommand {
        "accept" if positional.len() <= 1 => (
            "dialog.accept",
            serde_json::json!({"text": positional.first().map(String::as_str).unwrap_or("")}),
        ),
        "accept" => {
            return Err(ParseError {
                message: format!("accepts at most 1 arg(s), received {}", positional.len()),
                exit_code: 2,
            });
        }
        "auto" if positional.len() == 1 => {
            ("dialog.auto", serde_json::json!({"mode": positional[0]}))
        }
        "auto" => {
            return Err(ParseError {
                message: format!("accepts 1 arg(s), received {}", positional.len()),
                exit_code: 2,
            });
        }
        "dismiss" | "status" if positional.is_empty() => (
            if subcommand == "dismiss" {
                "dialog.dismiss"
            } else {
                "dialog.status"
            },
            serde_json::json!({}),
        ),
        "dismiss" | "status" => {
            return Err(ParseError {
                message: format!(
                    "unknown command {:?} for \"symbrowse dialog {subcommand}\"",
                    positional[0]
                ),
                exit_code: 2,
            });
        }
        _ => unreachable!("subcommand is selected from the supported dialog list"),
    };
    Ok(Action::Dispatch {
        session,
        command: command.to_owned(),
        args,
        format,
    })
}

fn parse_tab(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut session = String::from("default");
    let mut subcommand = None;
    let mut subcommand_index = None;
    let mut window_child_index = None;
    let mut scan = command_index + 1;
    while scan < values.len() {
        let value = values[scan].as_str();
        if subcommand.is_none() {
            match value {
                "list" | "new" | "switch" | "close" => {
                    subcommand = Some(value);
                    subcommand_index = Some(scan);
                    break;
                }
                "window" => {
                    subcommand = Some(value);
                    subcommand_index = Some(scan);
                }
                "--session" => {
                    session = required_value(values, scan + 1, "--session")?.to_owned();
                    scan += 1;
                }
                "--output" => {
                    format = parse_format(required_value(values, scan + 1, "--output")?)?;
                    scan += 1;
                }
                "--json" => {}
                value if value.starts_with("--json=") => {
                    json = parse_bool("--json", &value[7..])?;
                }
                value if value.starts_with("--output=") => {
                    format = parse_format(&value[9..])?;
                }
                value if value.starts_with("--session=") => session = value[10..].to_owned(),
                value if value.starts_with('-') => return Err(unknown_flag(value)),
                _ => {
                    return Ok(Action::Help(
                        help_catalog::help("tab", &[]).unwrap().to_owned(),
                    ));
                }
            }
        } else if subcommand == Some("window") && window_child_index.is_none() {
            match value {
                "window" => {
                    window_child_index = Some(scan);
                    break;
                }
                "--session" => {
                    session = required_value(values, scan + 1, "--session")?.to_owned();
                    scan += 1;
                }
                "--output" => {
                    format = parse_format(required_value(values, scan + 1, "--output")?)?;
                    scan += 1;
                }
                "--json" => {}
                value if value.starts_with("--json=") => {
                    json = parse_bool("--json", &value[7..])?;
                }
                value if value.starts_with("--output=") => {
                    format = parse_format(&value[9..])?;
                }
                value if value.starts_with("--session=") => session = value[10..].to_owned(),
                value if value.starts_with('-') => return Err(unknown_flag(value)),
                _ => {
                    return Ok(Action::Help(
                        help_catalog::help("tab", &["window"]).unwrap().to_owned(),
                    ));
                }
            }
        }
        scan += 1;
    }
    let Some(subcommand) = subcommand else {
        return Ok(Action::Help(
            help_catalog::help("tab", &[]).unwrap().to_owned(),
        ));
    };
    if subcommand == "window" && window_child_index.is_none() {
        return Ok(Action::Help(
            help_catalog::help("tab", &["window"]).unwrap().to_owned(),
        ));
    }

    let mut positional = Vec::new();
    let mut label = String::new();
    let mut positional_only = false;
    let mut index = command_index + 1;
    while index < values.len() {
        if Some(index) == subcommand_index || Some(index) == window_child_index {
            index += 1;
            continue;
        }
        let value = &values[index];
        if positional_only {
            positional.push(value.clone());
            index += 1;
            continue;
        }
        match value.as_str() {
            "--" => positional_only = true,
            "--json" => json = true,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            "--label" if subcommand == "new" => {
                index += 1;
                label = required_value(values, index, "--label")?.to_owned();
            }
            value if value.starts_with("--json=") => {
                json = parse_bool("--json", &value[7..])?;
            }
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with("--label=") && subcommand == "new" => {
                label = value[8..].to_owned();
            }
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            _ => positional.push(value.clone()),
        }
        index += 1;
    }
    if json {
        format = Format::Json;
    }

    let (command, args) = match subcommand {
        "list" if positional.is_empty() => ("tab.list", serde_json::json!({})),
        "new" if positional.len() <= 1 => (
            "tab.new",
            serde_json::json!({
                "label": label,
                "url": positional.first().map(String::as_str).unwrap_or("")
            }),
        ),
        "switch" if positional.len() == 1 => {
            ("tab.switch", serde_json::json!({"tab": positional[0]}))
        }
        "close" if positional.len() <= 1 => (
            "tab.close",
            serde_json::json!({"tab": positional.first().map(String::as_str).unwrap_or("")}),
        ),
        "window" if window_child_index.is_some() && positional.is_empty() => {
            ("window.new", serde_json::json!({}))
        }
        "list" | "window" => {
            let target = if subcommand == "window" {
                "symbrowse tab window window"
            } else {
                "symbrowse tab list"
            };
            return Err(ParseError {
                message: format!("unknown command {:?} for {target:?}", positional[0]),
                exit_code: 2,
            });
        }
        "new" | "close" => {
            return Err(ParseError {
                message: format!("accepts at most 1 arg(s), received {}", positional.len()),
                exit_code: 2,
            });
        }
        "switch" => {
            return Err(ParseError {
                message: format!("accepts 1 arg(s), received {}", positional.len()),
                exit_code: 2,
            });
        }
        _ => unreachable!("subcommand is selected from the supported tab list"),
    };
    Ok(Action::Dispatch {
        session,
        command: command.to_owned(),
        args,
        format,
    })
}

fn parse_frame(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut session = String::from("default");
    let mut subcommand_index = None;
    let mut scan = command_index + 1;
    while scan < values.len() {
        match values[scan].as_str() {
            "tree" => {
                subcommand_index = Some(scan);
                break;
            }
            "--session" => {
                session = required_value(values, scan + 1, "--session")?.to_owned();
                scan += 1;
            }
            "--output" => {
                format = parse_format(required_value(values, scan + 1, "--output")?)?;
                scan += 1;
            }
            "--json" => {}
            value if value.starts_with("--json=") => {
                json = parse_bool("--json", &value[7..])?;
            }
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            _ => {
                return Err(ParseError {
                    message: format!("unknown command {:?} for \"symbrowse frame\"", values[scan]),
                    exit_code: 2,
                });
            }
        }
        scan += 1;
    }
    let Some(subcommand_index) = subcommand_index else {
        return Ok(Action::Help(
            help_catalog::help("frame", &[]).unwrap().to_owned(),
        ));
    };
    let mut positional = Vec::new();
    let mut positional_only = false;
    let mut index = command_index + 1;
    while index < values.len() {
        if index == subcommand_index {
            index += 1;
            continue;
        }
        let value = &values[index];
        if positional_only {
            positional.push(value.clone());
            index += 1;
            continue;
        }
        match value.as_str() {
            "--" => positional_only = true,
            "--json" => json = true,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            _ => positional.push(value.clone()),
        }
        index += 1;
    }
    if json {
        format = Format::Json;
    }
    if let Some(value) = positional.first() {
        return Err(ParseError {
            message: format!("unknown command {value:?} for \"symbrowse frame tree\""),
            exit_code: 2,
        });
    }
    Ok(Action::Dispatch {
        session,
        command: "frame.tree".to_owned(),
        args: serde_json::json!({}),
        format,
    })
}

#[cfg(test)]
mod set_tests {
    use super::{Action, Format, parse};
    use std::ffi::OsString;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn set_offline_maps_only_the_runtime_supported_setting() {
        for (argv, offline, format, session) in [
            (&["set", "offline"][..], true, Format::Text, "default"),
            (&["set", "offline", "on"][..], true, Format::Text, "default"),
            (
                &["set", "offline", "off", "--json"][..],
                false,
                Format::Json,
                "default",
            ),
            (
                &["--output=yaml", "set", "offline", "off", "--session=x"][..],
                false,
                Format::Yaml,
                "x",
            ),
            (
                &["set", "offline", "on", "--json=false", "--output=yaml"][..],
                true,
                Format::Yaml,
                "default",
            ),
        ] {
            assert_eq!(
                parse(&args(argv)),
                Ok(Action::Dispatch {
                    session: session.into(),
                    command: "network.offline".into(),
                    args: serde_json::json!({"offline": offline}),
                    format,
                }),
                "argv={argv:?}"
            );
        }
    }

    #[test]
    fn set_offline_rejects_unknown_values_and_commands() {
        for argv in [
            &["set", "offline", "maybe"][..],
            &["set", "offline", "on", "off"][..],
            &["set", "viewport"][..],
        ] {
            assert!(parse(&args(argv)).is_err(), "argv={argv:?}");
        }
    }

    #[test]
    fn set_help_lists_supported_emulation_subcommands() {
        let Ok(Action::Help(parent)) = parse(&args(&["set", "--help"])) else {
            panic!("set parent help")
        };
        for supported in [
            "device",
            "geo",
            "headers",
            "media",
            "offline",
            "user-agent",
            "viewport",
        ] {
            assert!(parent.contains(supported), "help omitted {supported}");
            assert!(matches!(
                parse(&args(&["set", supported, "--help"])),
                Ok(Action::Help(_))
            ));
        }
        let Ok(Action::Help(leaf)) = parse(&args(&["set", "offline", "--help"])) else {
            panic!("set offline help")
        };
        assert!(leaf.contains("symbrowse set offline [on|off] [flags]"));
    }

    #[test]
    fn set_supported_emulation_commands_build_daemon_frames() {
        for (argv, command, expected_args) in [
            (
                &[
                    "--output=yaml",
                    "set",
                    "viewport",
                    "1280",
                    "720",
                    "2",
                    "--session=work",
                ][..],
                "set.viewport",
                serde_json::json!({"width":1280,"height":720,"scale":2.0}),
            ),
            (
                &["set", "device", "iPhone SE"][..],
                "set.device",
                serde_json::json!({"name":"iPhone SE"}),
            ),
            (
                &["set", "geo", "52.5", "13.4"][..],
                "set.geo",
                serde_json::json!({"latitude":52.5,"longitude":13.4}),
            ),
            (
                &["set", "headers", r#"{"x-test":"ok"}"#][..],
                "set.headers",
                serde_json::json!({"headers":{"x-test":"ok"}}),
            ),
            (
                &["set", "media", "dark"][..],
                "set.media",
                serde_json::json!({"dark":true}),
            ),
            (
                &["set", "user-agent", "fixture-agent"][..],
                "set.user-agent",
                serde_json::json!({"user_agent":"fixture-agent"}),
            ),
        ] {
            let Ok(Action::Dispatch {
                command: actual_command,
                args,
                ..
            }) = parse(&args(argv))
            else {
                panic!("expected dispatch for {argv:?}")
            };
            assert_eq!(actual_command, command, "argv={argv:?}");
            assert_eq!(args, expected_args, "argv={argv:?}");
        }
    }

    #[test]
    fn set_supported_emulation_commands_reject_invalid_payloads() {
        for argv in [
            &["set", "viewport", "wide", "720"][..],
            &["set", "geo", "north", "east"][..],
            &["set", "headers", "[]"][..],
            &["set", "headers", "{}"][..],
            &["set", "media", "auto"][..],
        ] {
            assert!(parse(&args(argv)).is_err(), "argv={argv:?}");
        }
    }

    #[test]
    fn set_device_list_uses_embedded_sorted_profile_names() {
        let Ok(Action::SetDeviceList { format }) = parse(&args(&["set", "device", "--list"]))
        else {
            panic!("device list action")
        };
        assert_eq!(format, Format::Text);
        let names = super::device_profile_names().expect("embedded profiles parse");
        assert_eq!(names.len(), 8);
        assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(names.contains(&"iPhone SE".to_owned()));
    }
}

fn parse_set(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let subcommand = values.get(command_index + 1).map(String::as_str);
    if subcommand != Some("offline") {
        return parse_set_dispatch(
            values,
            command_index,
            subcommand.ok_or_else(|| ParseError {
                message: "unknown command for \"set\"".into(),
                exit_code: 2,
            })?,
        );
    }
    let mut session = String::from("default");
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut offline = true;
    let mut has_value = false;
    let mut positional_count = 0usize;
    let mut index = command_index + 2;
    let mut positional_only = false;
    while index < values.len() {
        let value = &values[index];
        if positional_only {
            if has_value {
                return Err(ParseError {
                    message: format!(
                        "accepts at most 1 arg(s), received {}",
                        positional_count + 1
                    ),
                    exit_code: 2,
                });
            }
            offline = parse_offline(value)?;
            has_value = true;
            positional_count += 1;
            index += 1;
            continue;
        }
        match value.as_str() {
            "--" => positional_only = true,
            "--json" => json = true,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            _ if has_value => {
                return Err(ParseError {
                    message: format!(
                        "accepts at most 1 arg(s), received {}",
                        positional_count + 1
                    ),
                    exit_code: 2,
                });
            }
            value => {
                offline = parse_offline(value)?;
                has_value = true;
                positional_count += 1;
            }
        }
        index += 1;
    }
    if json {
        format = Format::Json;
    }
    Ok(Action::Dispatch {
        session,
        command: "network.offline".into(),
        args: serde_json::json!({"offline": offline}),
        format,
    })
}

fn parse_set_dispatch(
    values: &[String],
    command_index: usize,
    subcommand: &str,
) -> Result<Action, ParseError> {
    let (mut format, mut json) = root_output_flags(&values[..command_index])?;
    let mut session = String::from("default");
    let mut positionals = Vec::new();
    let mut list_devices = false;
    let mut index = command_index + 2;
    while index < values.len() {
        let value = &values[index];
        match value.as_str() {
            "--json" => json = true,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            "--list" if subcommand == "device" => list_devices = true,
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            _ => positionals.push(value.clone()),
        }
        index += 1;
    }
    if json {
        format = Format::Json;
    }
    if list_devices {
        if !positionals.is_empty() {
            return Err(ParseError {
                message: "set device --list accepts no device name".into(),
                exit_code: 2,
            });
        }
        return Ok(Action::SetDeviceList { format });
    }
    let (command, args) = match subcommand {
        "device" if positionals.len() == 1 => (
            "set.device",
            serde_json::json!({"name": positionals.remove(0)}),
        ),
        "viewport" if (2..=3).contains(&positionals.len()) => {
            let width = parse_set_number("width", &positionals[0])?;
            let height = parse_set_number("height", &positionals[1])?;
            let scale = positionals
                .get(2)
                .map_or(Ok(0.0), |value| parse_set_float("scale", value))?;
            (
                "set.viewport",
                serde_json::json!({"width": width, "height": height, "scale": scale}),
            )
        }
        "geo" if positionals.len() == 2 => (
            "set.geo",
            serde_json::json!({
                "latitude": parse_set_float("latitude", &positionals[0])?,
                "longitude": parse_set_float("longitude", &positionals[1])?
            }),
        ),
        "headers" if positionals.len() == 1 => {
            let parsed: serde_json::Value =
                serde_json::from_str(&positionals[0]).map_err(|e| ParseError {
                    message: format!("headers must be a JSON object: {e}"),
                    exit_code: 2,
                })?;
            let Some(map) = parsed.as_object() else {
                return Err(ParseError {
                    message: "headers must be a JSON object".into(),
                    exit_code: 2,
                });
            };
            if map.is_empty() || map.values().any(|value| !value.is_string()) {
                return Err(ParseError {
                    message: "headers object must contain string values".into(),
                    exit_code: 2,
                });
            }
            ("set.headers", serde_json::json!({"headers": parsed}))
        }
        "media" if positionals.len() == 1 => {
            let dark = match positionals[0].to_ascii_lowercase().as_str() {
                "dark" => true,
                "light" => false,
                _ => {
                    return Err(ParseError {
                        message: format!("media expects dark or light, got {:?}", positionals[0]),
                        exit_code: 2,
                    });
                }
            };
            ("set.media", serde_json::json!({"dark": dark}))
        }
        "user-agent" if positionals.len() == 1 => (
            "set.user-agent",
            serde_json::json!({"user_agent": positionals.remove(0)}),
        ),
        known @ ("device" | "viewport" | "geo" | "headers" | "media" | "user-agent") => {
            return Err(ParseError {
                message: format!("invalid arguments for set {known}"),
                exit_code: 2,
            });
        }
        _ => {
            return Err(ParseError {
                message: format!("unknown command {subcommand:?} for \"symbrowse set\""),
                exit_code: 2,
            });
        }
    };
    Ok(Action::Dispatch {
        session,
        command: command.into(),
        args,
        format,
    })
}

fn parse_set_number(name: &str, value: &str) -> Result<i64, ParseError> {
    value.parse().map_err(|error| ParseError {
        message: format!("{name}: {error}"),
        exit_code: 2,
    })
}

fn parse_set_float(name: &str, value: &str) -> Result<f64, ParseError> {
    value.parse().map_err(|error| ParseError {
        message: format!("{name}: {error}"),
        exit_code: 2,
    })
}

fn parse_offline(value: &str) -> Result<bool, ParseError> {
    match value.to_ascii_lowercase().as_str() {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(ParseError {
            message: format!("offline expects on or off, got {value:?}"),
            exit_code: 2,
        }),
    }
}

fn parse_flow(values: &[String], index: usize) -> Result<Action, ParseError> {
    let (root_format, mut json) = root_output_flags(&values[..index])?;
    let mut output = match root_format {
        Format::Text => "text",
        Format::Json => "json",
        Format::Yaml => "yaml",
    }
    .to_owned();
    let mut subcommand = "list";
    let mut subcommand_index = None;
    let mut scan = index + 1;
    while scan < values.len() {
        match values[scan].as_str() {
            "list" | "run" | "validate" => {
                subcommand = values[scan].as_str();
                subcommand_index = Some(scan);
                break;
            }
            "--output" | "--session" => scan += 1,
            "--json" | "--dry-run" => {}
            value
                if value.starts_with("--json=")
                    || value.starts_with("--output=")
                    || value.starts_with("--session=") => {}
            _ if values[scan].starts_with('-') => {}
            value => {
                return Err(ParseError {
                    message: format!("unknown command {value:?} for flow"),
                    exit_code: 2,
                });
            }
        }
        scan += 1;
    }
    let mut path = None;
    let mut session = "default".to_owned();
    let mut dry_run = false;
    let mut inputs = BTreeMap::new();
    let mut i = index + 1;
    let mut positional_only = false;
    while i < values.len() {
        if Some(i) == subcommand_index {
            i += 1;
            continue;
        }
        if positional_only {
            return Err(ParseError {
                message: format!(
                    "unknown command {:?} for \"symbrowse flow {subcommand}\"",
                    values[i]
                ),
                exit_code: 2,
            });
        }
        match values[i].as_str() {
            "--" => positional_only = true,
            "--json" => json = true,
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            "--dry-run" => dry_run = true,
            "--output" => {
                i += 1;
                output = required_value(values, i, "--output")?.to_owned();
            }
            value if value.starts_with("--output=") => output = value[9..].to_owned(),
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
                return Err(unknown_flag(value));
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
    let format = if json {
        Format::Json
    } else {
        parse_format(&output)?
    };
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
                return Err(unknown_flag(value));
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
                return Err(unknown_flag(value));
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
        && !matches!(
            value,
            "chrome" | "firefox" | "static" | "safari-attach" | "safari-bidi"
        )
    {
        return Err(ParseError {
            message: format!(
                "invalid engine {value:?}: use one of chrome, firefox, static, safari-attach, safari-bidi"
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
    let (mut format, mut json) = root_output_flags(&values[..state_index])?;
    let mut session = "default".to_owned();
    let mut names = Vec::new();
    let mut older_than = None;
    let mut index = state_index + 2;
    let mut positional_only = false;
    while index < values.len() {
        if positional_only {
            names.push(values[index].clone());
            index += 1;
            continue;
        }
        match values[index].as_str() {
            "--" => positional_only = true,
            "--json" => json = true,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
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
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            value if value.starts_with('-') => {
                return Err(unknown_flag(value));
            }
            value => names.push(value.to_owned()),
        }
        index += 1;
    }
    if json {
        format = Format::Json;
    }
    let command = match subcommand {
        "save" | "load" | "show" | "clear" | "list" | "clean" => format!("state.{subcommand}"),
        _ => {
            return Err(ParseError {
                message: format!("unknown command {subcommand:?} for state"),
                exit_code: 2,
            });
        }
    };
    let expected = if matches!(subcommand, "save" | "load" | "show" | "clear") {
        1
    } else {
        0
    };
    if names.len() != expected {
        return Err(ParseError {
            message: format!("accepts {expected} arg(s), received {}", names.len()),
            exit_code: 2,
        });
    }
    Ok(Action::StateOperation {
        session,
        command,
        name: names.into_iter().next(),
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
            _ if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            "--output" => {
                index += 1;
                output = required_value(values, index, "--output")?.to_owned();
            }
            _ if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            _ if value.starts_with("--output=") => output = value[9..].to_owned(),
            _ if value.starts_with('-') => {
                return Err(unknown_flag(value));
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
                return Err(unknown_flag(value));
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
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            "--output" => {
                index += 1;
                output = required_value(values, index, "--output")?.to_owned();
            }
            _ if value.starts_with("--output=") => output = value[9..].to_owned(),
            _ if value.starts_with('-') => return Err(unknown_flag(value)),
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
    let (mut format, mut json) = root_output_flags(&values[..upgrade_index])?;
    let mut check = false;
    let mut index = upgrade_index + 1;
    while index < values.len() {
        match values[index].as_str() {
            "--check" => check = true,
            "--json" => json = true,
            value if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            value if value.starts_with('-') => return Err(unknown_flag(value)),
            extra => {
                return Err(ParseError {
                    message: format!("unknown command {extra:?} for \"symbrowse upgrade\""),
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
    if json {
        format = Format::Json;
    }
    Ok(Action::UpgradeCheck { format })
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
    let mut positional_only = false;
    while index < values.len() {
        if index == config_index || index == show_index {
            index += 1;
            continue;
        }
        let value = &values[index];
        if value == "--" {
            positional_only = true;
            index += 1;
            continue;
        }
        if positional_only {
            return Err(ParseError {
                message: format!("unknown command {value:?} for \"symbrowse config show\""),
                exit_code: 2,
            });
        }
        match value.as_str() {
            "--json" => json = true,
            _ if value.starts_with("--json=") => json = parse_bool("--json", &value[7..])?,
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
                return Err(if value.starts_with('-') {
                    unknown_flag(value)
                } else {
                    ParseError {
                        message: format!("unknown command {value:?} for \"symbrowse config show\""),
                        exit_code: 2,
                    }
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
            exit_code: 1,
        })
}

fn unknown_flag(value: &str) -> ParseError {
    ParseError {
        message: format!("unknown flag: {value}"),
        exit_code: 1,
    }
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
    use super::{
        Action, Format, KeyInitResult, ParseError, SessionIdInfo, go_json_html_escape,
        go_json_string, mask_cookie_list, parse, parse_cache_range, parse_curl_cookie_line,
        render_cookie_list_json, render_cookie_list_text, render_cookie_list_yaml,
        render_session_id_json, render_state_key_init, session_id_info, snapshot_tree_diff,
    };
    use crate::AuthLoginEnvelope;
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::time::Duration;
    use symbrowse_core::cache::Cache;

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
    fn parses_doctor_formats_and_fix_without_arguments() {
        assert_eq!(
            parse(&args(&["doctor", "--fix", "--output=yaml"])),
            Ok(Action::Doctor {
                format: Format::Yaml,
                fix: true,
            })
        );
        assert_eq!(
            parse(&args(&["--json", "doctor"])),
            Ok(Action::Doctor {
                format: Format::Json,
                fix: false,
            })
        );
        assert!(parse(&args(&["doctor", "extra"])).is_err());
    }

    #[test]
    fn parses_downloads_session_directory_and_output() {
        assert_eq!(
            parse(&args(&[
                "downloads",
                "--dir",
                "/tmp/downloads",
                "--session=work",
                "--output=json",
            ])),
            Ok(Action::Downloads {
                session: "work".to_owned(),
                dir: Some("/tmp/downloads".to_owned()),
                format: Format::Json,
            })
        );
        assert_eq!(
            parse(&args(&[
                "watch",
                "--take-over=false",
                "--reason=ignored while watching",
            ])),
            Ok(Action::Watch {
                session: "default".to_owned(),
                take_over: false,
                reason: "ignored while watching".to_owned(),
                format: Format::Text,
            })
        );
        assert_eq!(
            parse(&args(&["--json", "downloads"])),
            Ok(Action::Downloads {
                session: "default".to_owned(),
                dir: None,
                format: Format::Json,
            })
        );
        assert!(parse(&args(&["downloads", "extra"])).is_err());
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
    fn batch_text_fallback_uses_go_json_html_escaping() {
        let commands = vec!["snapshot".to_owned()];
        let report = super::batch::run(&commands, false, false, |_| super::batch::ItemOutput {
            stdout: "<&>\u{2028}\u{2029}\n".to_owned(),
            error: None,
        });
        let rendered = super::render_batch_text(&report).expect("render batch text");
        assert!(rendered.contains("\\u003c\\u0026\\u003e\\u2028\\u2029"));
        assert!(!rendered.contains("<&>"));
        assert!(rendered.ends_with('\n'));
    }

    #[test]
    fn batch_executes_help_and_preserves_item_status_and_order() {
        let commands = vec![
            "help".to_owned(),
            "version --json".to_owned(),
            "version extra".to_owned(),
        ];
        let report = super::batch::run(&commands, false, false, |argv| {
            let result = super::execute_batch_item(argv);
            super::batch::ItemOutput {
                stdout: result.stdout,
                error: result.error,
            }
        });

        assert_eq!(report.results.len(), 3);
        assert_eq!(report.results[0].command, "help");
        assert!(report.results[0].success);
        assert!(
            report.results[0]
                .data
                .as_ref()
                .and_then(serde_json::Value::as_str)
                .is_some_and(
                    |help| help.starts_with("symbrowse is the standalone command-line entrypoint")
                )
        );
        assert_eq!(report.results[1].command, "version --json");
        assert_eq!(
            report.results[1]
                .data
                .as_ref()
                .and_then(|value| value.get("tool")),
            Some(&serde_json::json!("symbrowse"))
        );
        assert_eq!(report.results[2].command, "version extra");
        assert!(!report.results[2].success);
        assert_eq!(
            report.results[2].error.as_deref(),
            Some("unknown command \"extra\" for \"symbrowse version\"")
        );
        assert!(!report.bailed);
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
    fn state_inherits_root_output_flags_and_obeys_argument_separator() {
        assert_eq!(
            parse(&args(&["--json", "state", "save", "named"])),
            Ok(Action::StateOperation {
                session: "default".to_owned(),
                command: "state.save".to_owned(),
                name: Some("named".to_owned()),
                older_than: None,
                format: Format::Json,
            })
        );
        assert_eq!(
            parse(&args(&["--output=yaml", "state", "save", "named"])),
            Ok(Action::StateOperation {
                session: "default".to_owned(),
                command: "state.save".to_owned(),
                name: Some("named".to_owned()),
                older_than: None,
                format: Format::Yaml,
            })
        );
        assert_eq!(
            parse(&args(&["state", "save", "--", "--json"])),
            Ok(Action::StateOperation {
                session: "default".to_owned(),
                command: "state.save".to_owned(),
                name: Some("--json".to_owned()),
                older_than: None,
                format: Format::Text,
            })
        );
        assert_eq!(
            parse(&args(&["state", "save", "--", "one", "two"])),
            Err(ParseError {
                message: "accepts 1 arg(s), received 2".to_owned(),
                exit_code: 2,
            })
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
    fn oob_status_routes_to_the_daemon_with_global_flags() {
        assert_eq!(
            parse(&args(&["--json", "oob", "status", "--session", "fixture"])),
            Ok(Action::Dispatch {
                session: "fixture".into(),
                command: "oob.status".into(),
                args: serde_json::json!({}),
                format: Format::Json,
            })
        );
    }

    #[test]
    fn handoff_requires_reason_and_forwards_the_timeout() {
        assert_eq!(
            parse(&args(&["handoff", "--reason", "2FA", "--timeout", "1m"])),
            Ok(Action::Dispatch {
                session: "default".into(),
                command: "handoff".into(),
                args: serde_json::json!({"reason":"2FA", "timeout":"1m"}),
                format: Format::Text,
            })
        );
        assert!(parse(&args(&["handoff"])).is_err());
    }

    #[test]
    fn mcp_rejects_unknown_engine_like_go() {
        let error = parse(&args(&["mcp", "--engine", "netscape"]))
            .expect_err("unknown engine must fail before stdio starts");
        assert_eq!(error.exit_code, 1);
        assert_eq!(
            error.message,
            "invalid engine \"netscape\": use one of chrome, firefox, static, safari-attach, safari-bidi"
        );
    }

    #[test]
    fn cobra_flag_parse_errors_use_failure_exit_code() {
        for (values, expected) in [
            (&["--unknown", "version"][..], "unknown flag: --unknown"),
            (&["version", "--unknown"][..], "unknown flag: --unknown"),
            (
                &["config", "show", "--config-dir"][..],
                "flag needs an argument: --config-dir",
            ),
            (&["mcp", "--engine"][..], "flag needs an argument: --engine"),
        ] {
            assert_eq!(
                parse(&args(values)),
                Err(ParseError {
                    message: expected.to_owned(),
                    exit_code: 1,
                }),
                "args={values:?}"
            );
        }
    }

    #[test]
    fn mcp_accepts_firefox_engine() {
        assert_eq!(super::validate_engine("firefox"), Ok(()));
        assert!(matches!(
            parse(&args(&["mcp", "--engine", "firefox"])),
            Ok(Action::Mcp { engine: Some(engine), .. }) if engine == "firefox"
        ));
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
    fn dialog_commands_dispatch_go_payloads_and_match_argument_rules() {
        for (argv, expected_command, expected_args) in [
            (
                &["dialog", "status"][..],
                "dialog.status",
                serde_json::json!({}),
            ),
            (
                &["dialog", "dismiss"][..],
                "dialog.dismiss",
                serde_json::json!({}),
            ),
            (
                &["dialog", "accept", "prompt text"][..],
                "dialog.accept",
                serde_json::json!({"text": "prompt text"}),
            ),
            (
                &["dialog", "auto", "accept"][..],
                "dialog.auto",
                serde_json::json!({"mode": "accept"}),
            ),
        ] {
            let Action::Dispatch { command, args, .. } =
                parse(&args(argv)).expect("dialog dispatch")
            else {
                panic!("wrong action for {argv:?}");
            };
            assert_eq!(command, expected_command, "argv={argv:?}");
            assert_eq!(args, expected_args, "argv={argv:?}");
        }

        assert_eq!(
            parse(&args(&["dialog", "--session", "work", "status"])),
            Ok(Action::Dispatch {
                session: "work".to_owned(),
                command: "dialog.status".to_owned(),
                args: serde_json::json!({}),
                format: Format::Text,
            })
        );
        assert_eq!(
            parse(&args(&["dialog", "--session"])),
            Err(ParseError {
                message: "flag needs an argument: --session".to_owned(),
                exit_code: 1,
            })
        );
        assert!(matches!(parse(&args(&["dialog"])), Ok(Action::Help(_))));
        assert!(parse(&args(&["dialog", "auto"])).is_err());
        assert!(parse(&args(&["dialog", "status", "extra"])).is_err());
    }

    #[test]
    fn storage_get_dispatches_read_only_origin_scoped_requests() {
        assert_eq!(
            parse(&args(&[
                "--output",
                "json",
                "storage",
                "--session=work",
                "get",
                "session",
                "step"
            ])),
            Ok(Action::StorageGet {
                session: "work".to_owned(),
                kind: "session".to_owned(),
                key: Some("step".to_owned()),
                format: Format::Json,
            })
        );
        assert_eq!(
            parse(&args(&["storage", "get", "local", "--", "--json"])),
            Ok(Action::StorageGet {
                session: "default".to_owned(),
                kind: "local".to_owned(),
                key: Some("--json".to_owned()),
                format: Format::Text,
            })
        );
        assert!(parse(&args(&["storage", "get"])).is_err());
        assert!(parse(&args(&["storage", "get", "local", "key", "extra"])).is_err());
        assert!(parse(&args(&["storage", "set", "local", "key"])).is_err());
        assert!(parse(&args(&["storage", "clear"])).is_err());
        let Action::Help(help) = parse(&args(&["storage", "--help"])).unwrap() else {
            panic!("storage help expected")
        };
        assert!(help.contains("get         Read web storage values"));
        assert!(help.contains("clear       Remove all web storage values"));
        assert!(help.contains("set         Write one web storage value"));
        let Action::Help(get_help) =
            parse(&args(&["storage", "--session", "work", "get", "--help"])).unwrap()
        else {
            panic!("storage get help after persistent flag expected")
        };
        assert!(get_help.contains("symbrowse storage get <local|session> [key]"));
        assert!(matches!(
            parse(&args(&["storage", "set", "session", "token", "secret-like-value"])),
            Ok(Action::Dispatch {
                command,
                args,
                ..
            }) if command == "storage.set"
                && args == serde_json::json!({"kind":"session","key":"token","value":"secret-like-value"})
        ));
        assert!(matches!(
            parse(&args(&["storage", "clear", "local", "--json"])),
            Ok(Action::Dispatch {
                command,
                args,
                format: Format::Json,
                ..
            }) if command == "storage.clear" && args == serde_json::json!({"kind":"local"})
        ));
        assert!(matches!(
            parse(&args(&[
                "storage",
                "--session",
                "work",
                "set",
                "local",
                "key",
                "value",
                "--help"
            ])),
            Ok(Action::Help(_))
        ));
    }

    #[test]
    fn session_id_parses_flags_and_preserves_go_json_order() {
        assert_eq!(
            parse(&args(&[
                "session",
                "--session",
                "ignored",
                "id",
                "--scope=repo",
                "--prefix",
                "agent",
                "--output=json"
            ])),
            Ok(Action::SessionId {
                scope: "repo".to_owned(),
                prefix: "agent".to_owned(),
                format: Format::Json,
            })
        );
        assert_eq!(
            parse(&args(&["session", "id", "--scope", "invalid"])),
            Ok(Action::SessionId {
                scope: "invalid".to_owned(),
                prefix: String::new(),
                format: Format::Text,
            })
        );
        assert!(parse(&args(&["session", "--scope=repo", "id"])).is_err());
        let hashed = session_id_info("repo", "agent", Path::new("/tmp/origin"), false);
        assert_eq!(hashed.id, "agent-28a1676df0916386");
        assert_eq!(hashed.origin_path, "/tmp/origin");
        let info = SessionIdInfo {
            id: "agent-1234567890abcdef".to_owned(),
            scope: "repo".to_owned(),
            prefix: "agent<&".to_owned(),
            origin_path: "/tmp/a&b".to_owned(),
            fallback: false,
        };
        assert_eq!(
            render_session_id_json(&info),
            "{\"success\":true,\"data\":{\"id\":\"agent-1234567890abcdef\",\"scope\":\"repo\",\"prefix\":\"agent<&\",\"origin_path\":\"/tmp/a&b\"}}\n"
        );
        assert_eq!(
            go_json_string("quote\" slash\\ newline\n"),
            "\"quote\\\" slash\\\\ newline\\n\""
        );
    }

    #[test]
    fn a11y_routes_tags_selector_and_optional_url() {
        let Action::Dispatch {
            session,
            command,
            args: payload,
            format,
        } = parse(&args(&[
            "a11y",
            "https://fixture.invalid",
            "--tags",
            "wcag2a, ,wcag2aa",
            "--selector",
            ".main",
            "--output=json",
        ]))
        .unwrap()
        else {
            panic!("a11y must dispatch");
        };
        assert_eq!(session, "default");
        assert_eq!(command, "a11y");
        assert_eq!(format, Format::Json);
        assert_eq!(
            payload,
            serde_json::json!({
                "url": "https://fixture.invalid",
                "tags": ["wcag2a", "wcag2aa"],
                "selector": ".main",
            })
        );

        let Action::Dispatch { args: defaults, .. } = parse(&args(&["a11y"])).unwrap() else {
            panic!("a11y must dispatch");
        };
        assert_eq!(defaults, serde_json::json!({"tags": null, "selector": ""}));
        assert!(parse(&args(&["a11y", "one", "two"])).is_err());
    }

    #[test]
    fn screenshot_routes_go_payload_and_argument_limits() {
        let Action::Dispatch {
            session,
            command,
            args: payload,
            format,
        } = parse(&args(&[
            "screenshot",
            "fixture.jpg",
            "--full",
            "--selector",
            "#hero",
            "--format",
            "jpeg",
            "--quality",
            "80",
            "--screenshot-dir",
            "/tmp/screens",
            "--json",
        ]))
        .unwrap()
        else {
            panic!("screenshot must dispatch");
        };
        assert_eq!(session, "default");
        assert_eq!(command, "screenshot");
        assert_eq!(format, Format::Json);
        assert_eq!(
            payload,
            serde_json::json!({
                "path": "fixture.jpg",
                "full": true,
                "selector": "#hero",
                "format": "jpeg",
                "quality": 80,
                "dir": "/tmp/screens",
            })
        );

        let Action::Dispatch { args: defaults, .. } = parse(&args(&["screenshot"])).unwrap() else {
            panic!("screenshot must dispatch");
        };
        assert_eq!(
            defaults,
            serde_json::json!({
                "full": false,
                "selector": "",
                "format": "png",
                "quality": 0,
                "dir": "",
            })
        );
        assert!(parse(&args(&["screenshot", "one", "two"])).is_err());
        assert!(parse(&args(&["screenshot", "--quality", "bad"])).is_err());
    }

    #[test]
    fn cache_get_parses_range_and_format_flags() {
        assert_eq!(
            parse(&args(&[
                "cache",
                "get",
                "out_0123456789ab",
                "--range=2-4",
                "--json"
            ])),
            Ok(Action::CacheGet {
                id: "out_0123456789ab".to_owned(),
                range: Some("2-4".to_owned()),
                format: Format::Json,
            })
        );
        assert_eq!(parse_cache_range("-3"), Ok((0, 3)));
        assert_eq!(parse_cache_range("2-"), Ok((2, 0)));
        assert_eq!(
            parse_cache_range("0-3"),
            Err("invalid range \"0-3\": start must be a positive line number".to_owned())
        );
        assert_eq!(
            parse(&args(&["cache", "clear", "--output=json"])),
            Ok(Action::CacheClear {
                format: Format::Json,
            })
        );
        assert_eq!(
            parse(&args(&["cache", "list"])),
            Ok(Action::CacheList {
                format: Format::Text,
            })
        );
        assert!(parse(&args(&["cache", "clear", "extra"])).is_err());
    }

    #[test]
    fn cache_clear_counts_and_removes_entries_from_both_cache_roots() {
        let root = tempfile::tempdir().expect("temp cache root");
        let output = Cache::new(root.path().join("out"), Duration::ZERO);
        output
            .store(b"truncated output")
            .expect("store output entry");
        let fetch = symbrowse_fetch::cache::ResponseCache::new(root.path().join("fetch"))
            .with_ttl(Duration::from_secs(3600));
        let key = "a".repeat(64);
        fetch
            .put(
                &key,
                b"response body",
                &serde_json::json!({
                    "stored_at": time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap(),
                    "ttl": 3_600_000_000_000_i64
                }),
            )
            .expect("store fetch entry");
        let expired_key = "b".repeat(64);
        fetch
            .put(
                &expired_key,
                b"expired response",
                &serde_json::json!({"stored_at": "2000-01-01T00:00:00Z", "ttl": 1}),
            )
            .expect("store expired fetch entry");

        let entries = super::cache_list_entries(
            &output,
            &root.path().join("fetch"),
            Duration::from_secs(3600),
        )
        .expect("list both cache roots");
        assert_eq!(entries.len(), 2, "expired fetch entries are omitted");
        assert_eq!(entries[0].kind, "output");
        assert_eq!(entries[1].id, format!("fetch:{key}"));
        assert_eq!(entries[1].kind, "fetch-response");
        assert_eq!(entries[1].bytes, b"response body".len() as u64);

        assert_eq!(
            super::clear_cache_entries(&output, &fetch, Duration::from_secs(3600)),
            Ok(2)
        );
        assert!(output.list().expect("output cache list").is_empty());
        assert!(matches!(
            fetch.get(&key),
            Err(symbrowse_fetch::cache::CacheError::NotFound(_))
        ));
    }

    #[test]
    fn tab_commands_dispatch_runtime_payloads_and_match_argument_rules() {
        for (argv, expected_command, expected_args) in [
            (&["tab", "list"][..], "tab.list", serde_json::json!({})),
            (
                &["tab", "new", "https://example.com", "--label", "docs"][..],
                "tab.new",
                serde_json::json!({"label": "docs", "url": "https://example.com"}),
            ),
            (
                &["tab", "switch", "t2"][..],
                "tab.switch",
                serde_json::json!({"tab": "t2"}),
            ),
            (
                &["tab", "close", "docs"][..],
                "tab.close",
                serde_json::json!({"tab": "docs"}),
            ),
            (
                &["tab", "window", "window"][..],
                "window.new",
                serde_json::json!({}),
            ),
        ] {
            let Action::Dispatch {
                command,
                args: payload,
                ..
            } = parse(&args(argv)).expect("tab dispatch")
            else {
                panic!("wrong action for {argv:?}");
            };
            assert_eq!(command, expected_command, "argv={argv:?}");
            assert_eq!(payload, expected_args, "argv={argv:?}");
        }

        assert_eq!(
            parse(&args(&["tab", "--session", "work", "list"])),
            Ok(Action::Dispatch {
                session: "work".to_owned(),
                command: "tab.list".to_owned(),
                args: serde_json::json!({}),
                format: Format::Text,
            })
        );
        assert!(matches!(parse(&args(&["tab"])), Ok(Action::Help(_))));
        assert!(matches!(
            parse(&args(&["tab", "window"])),
            Ok(Action::Help(_))
        ));
        assert!(parse(&args(&["tab", "switch"])).is_err());
        assert!(parse(&args(&["tab", "list", "extra"])).is_err());
    }

    #[test]
    fn implemented_interactions_dispatch_go_action_payloads() {
        for (argv, expected_command, expected_args) in [
            (
                &["check", "#agree"][..],
                "check",
                serde_json::json!({"action": "check", "selector": "#agree"}),
            ),
            (
                &["dblclick", "button.submit"][..],
                "dblclick",
                serde_json::json!({"action": "dblclick", "selector": "button.submit"}),
            ),
            (
                &["focus", "#search"][..],
                "focus",
                serde_json::json!({"action": "focus", "selector": "#search"}),
            ),
            (
                &["hover", "#menu"][..],
                "hover",
                serde_json::json!({"action": "hover", "selector": "#menu"}),
            ),
            (
                &["select", "#country", "DE"][..],
                "select",
                serde_json::json!({"action": "select", "selector": "#country", "value": "DE"}),
            ),
            (
                &["uncheck", "#newsletter"][..],
                "uncheck",
                serde_json::json!({"action": "uncheck", "selector": "#newsletter"}),
            ),
            (
                &["scrollintoview", "#target"][..],
                "scrollintoview",
                serde_json::json!({"action": "scrollintoview", "selector": "#target"}),
            ),
            (
                &["scroll", "#target", "--", "-240"][..],
                "scroll",
                serde_json::json!({"action": "scroll", "selector": "#target", "amount": -240}),
            ),
        ] {
            let Action::Dispatch {
                command,
                args: payload,
                ..
            } = parse(&args(argv)).expect("interaction dispatch")
            else {
                panic!("wrong action for {argv:?}");
            };
            assert_eq!(command, expected_command, "argv={argv:?}");
            assert_eq!(payload, expected_args, "argv={argv:?}");
        }
        assert_eq!(
            parse(&args(&["check"])),
            Err(ParseError {
                message: "check requires a selector and optional value".to_owned(),
                exit_code: 2,
            })
        );
        assert_eq!(
            parse(&args(&["check", "#agree", "extra"])),
            Err(ParseError {
                message: "check requires a selector and optional value".to_owned(),
                exit_code: 2,
            })
        );
        assert!(parse(&args(&["check", "--selector", "#agree"])).is_err());
    }

    #[test]
    fn frame_tree_is_the_only_advertised_frame_operation() {
        assert_eq!(
            parse(&args(&["frame", "tree", "--session", "work"])),
            Ok(Action::Dispatch {
                session: "work".to_owned(),
                command: "frame.tree".to_owned(),
                args: serde_json::json!({}),
                format: Format::Text,
            })
        );
        let Action::Help(parent) = parse(&args(&["frame"])).expect("frame help") else {
            panic!("wrong action");
        };
        assert!(parent.contains("tree        Show the nested frame tree"));
        assert!(!parent.contains("main        Address the main frame"));
        assert!(!parent.contains("select      Address a nested frame"));
        assert!(parse(&args(&["frame", "main"])).is_err());
        assert!(parse(&args(&["frame", "tree", "extra"])).is_err());
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

    #[test]
    fn upgrade_check_parses_only_read_only_flags_and_matches_help_catalog() {
        assert_eq!(
            parse(&args(&["--output=yaml", "upgrade", "--check", "--json"])),
            Ok(Action::UpgradeCheck {
                format: Format::Json
            })
        );
        assert!(parse(&args(&["upgrade"])).is_err());
        assert!(parse(&args(&["upgrade", "--apply"])).is_err());
        let Action::Help(help) = parse(&args(&["upgrade", "--help"])).unwrap() else {
            panic!("upgrade help action")
        };
        assert!(help.contains("symbrowse upgrade [flags]"));
        assert!(help.contains("--check   only check for updates, do not apply"));
        let Action::Help(root) = parse(&args(&["--help"])).unwrap() else {
            panic!("root help action")
        };
        for command in ["handoff", "oob", "upgrade"] {
            assert!(root.contains(command), "root help omitted {command}");
        }
    }

    #[test]
    fn eval_parser_preserves_go_input_and_inherited_output_flags() {
        let Action::Eval {
            expression,
            from_stdin,
            base64,
            format,
            ..
        } = parse(&args(&[
            "--json", "eval", "1+1", "ignored", "--stdin", "--base64",
        ]))
        .expect("eval parse")
        else {
            panic!("wrong eval action")
        };
        assert_eq!(expression.as_deref(), Some("1+1"));
        assert!(from_stdin);
        assert!(base64);
        assert_eq!(format, Format::Json);

        let Action::Eval {
            expression, format, ..
        } = parse(&args(&["--output", "yaml", "eval", "--output=text", "x"]))
            .expect("eval output flags")
        else {
            panic!("wrong eval action")
        };
        assert_eq!(expression.as_deref(), Some("x"));
        assert_eq!(format, Format::Text);
    }

    #[test]
    fn eval_input_resolution_matches_go_stdin_precedence_and_validation() {
        assert_eq!(
            super::resolve_eval_expression(Some("1+1".into()), false, b"ignored"),
            Some("1+1".into())
        );
        assert_eq!(
            super::resolve_eval_expression(Some("argv expression".into()), true, b"1+1"),
            Some("1+1".into())
        );
        assert_eq!(
            super::resolve_eval_expression(Some("argv expression".into()), true, b""),
            Some("".into())
        );
        assert_eq!(
            super::resolve_eval_expression(None, false, b"ignored"),
            None
        );

        // Go resolves stdin before --base64 decoding, even when argv is also
        // present; the Rust dispatch must feed that selected value onward.
        let selected =
            super::resolve_eval_expression(Some("YXJndg==".into()), true, b"ZG9jdW1lbnQudGl0bGU=")
                .expect("stdin expression");
        assert_eq!(
            super::decode_standard_base64(&selected),
            Ok(b"document.title".to_vec())
        );
    }

    #[test]
    fn standard_base64_decoder_matches_go_padding_and_newline_rules() {
        assert_eq!(
            super::decode_standard_base64("ZG9jdW1lbnQudGl0bGU="),
            Ok(b"document.title".to_vec())
        );
        assert_eq!(super::decode_standard_base64("MSsy\n"), Ok(b"1+2".to_vec()));
        assert_eq!(super::decode_standard_base64("AB=="), Ok(vec![0]));
        assert_eq!(super::decode_standard_base64("!!!"), Err(0));
        assert_eq!(super::decode_standard_base64("YQ="), Err(3));
        assert_eq!(super::decode_standard_base64("YQ==x"), Err(4));
        assert_eq!(super::decode_standard_base64("YQ=\n"), Err(4));
        assert_eq!(super::decode_standard_base64("AA==\nA"), Err(5));
    }

    #[test]
    fn downloads_help_matches_cobra_flag_order_and_padding() {
        let help = super::command_help("downloads", &[]).expect("downloads help");
        let dir = help
            .find("      --dir string       set the download directory first")
            .unwrap();
        let usage = help
            .find("  -h, --help             help for downloads")
            .unwrap();
        let session = help
            .find("      --session string   session name (default \"default\")")
            .unwrap();
        assert!(dir < usage && usage < session);
        assert_eq!(help.len(), 521);
    }

    #[test]
    fn help_lists_only_commands_with_rust_dispatch_paths() {
        let Action::Help(root) = parse(&args(&["--help"])).expect("root help") else {
            panic!("root help action")
        };
        for implemented in [
            "auth",
            "eval",
            "cookies",
            "compat-sidecar",
            "flow",
            "handoff",
            "open",
            "oob",
            "screenshot",
            "scroll",
            "state",
            "tab",
            "tools",
            "trace",
            "upgrade",
            "upload",
            "version",
            "watch",
            "workflow",
        ] {
            assert!(
                root.contains(implemented),
                "root help omitted {implemented}"
            );
            assert!(
                matches!(parse(&args(&[implemented, "--help"])), Ok(Action::Help(_))),
                "root help advertised {implemented} without a Rust help/dispatch path"
            );
        }
        for path in [
            &["auth", "login", "--help"][..],
            &["flow", "--help"][..],
            &["flow", "validate", "--help"][..],
            &["state", "key", "init", "--help"][..],
            &["tools", "list", "--help"][..],
            &["trace", "export", "--help"][..],
            &["config", "show", "--help"][..],
        ] {
            assert!(
                matches!(parse(&args(path)), Ok(Action::Help(_))),
                "{path:?}"
            );
        }
    }

    #[test]
    fn auth_login_sends_only_vault_reference_and_navigation_target() {
        assert_eq!(
            parse(&args(&[
                "auth",
                "login",
                "fixture-entry",
                "--url",
                "https://fixture.invalid/login",
                "--session",
                "fixture",
                "--json",
            ])),
            Ok(Action::Dispatch {
                session: "fixture".into(),
                command: "auth.login".into(),
                args: serde_json::json!({
                    "entry": "fixture-entry",
                    "url": "https://fixture.invalid/login"
                }),
                format: Format::Json,
            })
        );
        assert!(parse(&args(&["auth", "login"])).is_err());
        assert!(parse(&args(&["auth", "login", "one", "two"])).is_err());
        assert!(parse(&args(&["auth", "login", "fixture", "--password", "leak"])).is_err());
    }

    #[test]
    fn auth_login_json_data_keys_follow_go_map_order() {
        let mut data = serde_json::Map::new();
        for (key, value) in [
            ("status", serde_json::json!("ready")),
            ("url", serde_json::json!("https://fixture.invalid/login")),
            ("username_set", serde_json::json!(true)),
            ("password_set", serde_json::json!(true)),
        ] {
            data.insert(key.to_owned(), value);
        }
        let data = serde_json::Value::Object(data);
        let envelope = AuthLoginEnvelope {
            success: true,
            data: &data,
            warnings: Vec::new(),
        };
        assert_eq!(
            serde_json::to_string(&envelope).unwrap(),
            r#"{"success":true,"data":{"password_set":true,"status":"ready","url":"https://fixture.invalid/login","username_set":true}}"#
        );
    }

    #[test]
    fn help_command_routes_root_and_known_command_help() {
        let Action::Help(root) = parse(&args(&["help"])).expect("help root") else {
            panic!("help should print root help");
        };
        assert_eq!(root, super::root_help());
        assert!(root.contains("  help           Help about any command"));

        let Action::Help(usage) = parse(&args(&["help", "scroll"])).expect("scroll help") else {
            panic!("help scroll should print command help");
        };
        let Action::Help(direct_usage) =
            parse(&args(&["scroll", "--help"])).expect("direct scroll help")
        else {
            panic!("scroll --help should print command help");
        };
        assert_eq!(usage, direct_usage);

        let Action::Help(help_usage) =
            parse(&args(&["help", "--help"])).expect("help command usage")
        else {
            panic!("help --help should print help command usage");
        };
        assert_eq!(help_usage, super::help_command_help());
    }

    #[test]
    fn journal_cli_parses_tail_show_and_text_rows() {
        let Action::Dispatch {
            session,
            command,
            args: payload,
            ..
        } = parse(&args(&[
            "journal",
            "tail",
            "--lines=4",
            "--session",
            "alpha",
        ]))
        .expect("journal tail")
        else {
            panic!("journal tail should dispatch");
        };
        assert_eq!(session, "alpha");
        assert_eq!(command, "journal.tail");
        assert_eq!(payload, serde_json::json!({"session":"alpha", "lines":4}));
        let Action::Dispatch {
            command,
            args: payload,
            ..
        } = parse(&args(&["journal", "show"])).expect("journal show")
        else {
            panic!("journal show should dispatch");
        };
        assert_eq!(command, "journal.show");
        assert_eq!(payload, serde_json::json!({"session":"default"}));
        assert_eq!(
            super::render_journal_text(&serde_json::json!({"entries":[{
                "timestamp":"t", "command":"snapshot", "risk_class":"read",
                "decider":"policy", "result":"ok"
            }]})),
            "t\tsnapshot\tread\tpolicy\tok\n"
        );
        assert!(parse(&args(&["journal", "tail", "extra"])).is_err());
    }

    #[test]
    fn watch_cli_parses_read_only_stream_and_takeover() {
        assert_eq!(
            parse(&args(&["watch", "--session=agent", "--json"])),
            Ok(Action::Watch {
                session: "agent".to_owned(),
                take_over: false,
                reason: String::new(),
                format: Format::Json,
            })
        );
        assert_eq!(
            parse(&args(&[
                "watch",
                "--take-over",
                "--reason",
                "operator review",
                "--output=yaml",
                "--session=agent",
            ])),
            Ok(Action::Watch {
                session: "agent".to_owned(),
                take_over: true,
                reason: "operator review".to_owned(),
                format: Format::Yaml,
            })
        );
        assert!(parse(&args(&["watch", "extra"])).is_err());
        assert!(parse(&args(&["watch", "--take-over"])).is_err());
        assert!(parse(&args(&["watch", "--take-over", "--reason="])).is_err());
    }

    #[test]
    fn network_requests_rejects_extra_argument_as_unknown_subcommand() {
        assert_eq!(
            parse(&args(&["network", "requests", "extra"])),
            Err(ParseError {
                message: "unknown command \"extra\" for \"symbrowse network requests\"".to_owned(),
                exit_code: 2,
            })
        );
    }

    #[test]
    fn network_request_routes_id_and_output_format() {
        assert_eq!(
            parse(&args(&[
                "network",
                "request",
                "fixture-1",
                "--session",
                "fixture",
                "--json",
            ])),
            Ok(Action::Dispatch {
                session: "fixture".to_owned(),
                command: "network.request".to_owned(),
                args: serde_json::json!({"id": "fixture-1"}),
                format: Format::Json,
            })
        );
    }

    #[test]
    fn cli_daemon_frames_carry_request_metadata() {
        let frame = super::cli_frame(
            "trace.replay",
            "default",
            Some(serde_json::json!({"steps":[]})),
        );
        assert_eq!(frame.cmd, "trace.replay");
        assert!(!frame.request_id.is_empty());
        assert_eq!(frame.retrieval_surface, "cli");
    }

    #[test]
    fn trace_yaml_single_quote_escapes_quote_characters() {
        assert_eq!(super::yaml_single_quote("a: it's"), "'a: it''s'");
    }

    #[test]
    fn trace_export_cli_parses_session_output_and_file() {
        assert_eq!(
            parse(&args(&[
                "trace",
                "export",
                "--session=fixture",
                "--out",
                "fixture.json",
                "--json"
            ])),
            Ok(Action::TraceExport {
                session: "fixture".into(),
                path: PathBuf::from("fixture.json"),
                format: Format::Json,
            })
        );
        assert!(parse(&args(&["trace", "export", "extra"])).is_err());
    }

    #[test]
    fn trace_replay_cli_parses_one_file_and_output_flags() {
        assert_eq!(
            parse(&args(&[
                "trace",
                "replay",
                "fixture.json",
                "--session=fixture",
                "--json"
            ])),
            Ok(Action::TraceReplay {
                session: "fixture".into(),
                path: PathBuf::from("fixture.json"),
                format: Format::Json,
            })
        );
        assert!(parse(&args(&["trace", "replay"])).is_err());
        assert!(parse(&args(&["trace", "replay", "one", "two"])).is_err());
    }

    #[test]
    fn trace_export_omits_empty_optional_step_fields_like_go() {
        let output = super::TraceFileOutput {
            schema_version: 1,
            created_at: "2026-09-25T00:00:00Z".into(),
            session: "fixture".into(),
            steps: vec![super::TraceStepOutput {
                command: "open".into(),
                selector: String::new(),
                value: String::new(),
                key: String::new(),
                url: "https://fixture.invalid".into(),
                expected_url: "https://fixture.invalid".into(),
            }],
        };
        assert_eq!(
            serde_json::to_string(&output).expect("serialize trace"),
            r#"{"schema_version":1,"created_at":"2026-09-25T00:00:00Z","session":"fixture","steps":[{"command":"open","url":"https://fixture.invalid","expected_url":"https://fixture.invalid"}]}"#
        );
        assert_eq!(
            go_json_html_escape("https://fixture.invalid/?a=1&b=<x>".into()),
            "https://fixture.invalid/?a=1\\u0026b=\\u003cx\\u003e"
        );
    }

    #[test]
    fn diff_snapshot_cli_parses_baseline_and_session() {
        assert_eq!(
            parse(&args(&[
                "diff",
                "--session",
                "fixture",
                "snapshot",
                "--baseline=before.json",
            ])),
            Ok(Action::DiffSnapshot {
                session: "fixture".into(),
                baseline: Some(PathBuf::from("before.json")),
                format: Format::Text,
            })
        );
        assert!(parse(&args(&["diff", "snapshot", "extra"])).is_err());
    }

    #[test]
    fn diff_url_cli_requires_two_targets_and_preserves_format() {
        assert_eq!(
            parse(&args(&[
                "diff",
                "url",
                "https://before.invalid",
                "https://after.invalid",
                "--session=fixture",
                "--json",
            ])),
            Ok(Action::DiffUrl {
                session: "fixture".into(),
                first_url: "https://before.invalid".into(),
                second_url: "https://after.invalid".into(),
                format: Format::Json,
            })
        );
        assert!(parse(&args(&["diff", "url", "https://only.invalid"])).is_err());
    }

    #[test]
    fn diff_snapshot_baseline_lines_match_go_set_comparison() {
        assert_eq!(
            snapshot_tree_diff("before\nshared\n", "shared\nafter\n"),
            serde_json::json!({
                "added": ["after"],
                "diff": ["- before", "+ after"],
                "removed": ["before"],
                "stable": 1,
            })
        );
    }

    #[test]
    fn policy_explain_cli_dispatches_command_url_mode_and_session() {
        let Action::Dispatch {
            session,
            command,
            args: payload,
            format,
        } = parse(&args(&[
            "policy",
            "explain",
            "snapshot",
            "--url",
            "https://example.invalid/path",
            "--mode=mcp",
            "--session",
            "research",
            "--json",
        ]))
        .expect("policy explain")
        else {
            panic!("policy explain should dispatch");
        };
        assert_eq!(session, "research");
        assert_eq!(command, "policy.explain");
        assert_eq!(format, Format::Json);
        assert_eq!(
            payload,
            serde_json::json!({
                "command":"snapshot", "url":"https://example.invalid/path", "mode":"mcp"
            })
        );
        assert!(parse(&args(&["policy", "explain"])).is_err());
        assert!(parse(&args(&["policy", "explain", "snapshot", "extra"])).is_err());
    }

    #[test]
    fn upload_cli_preserves_file_arguments_without_client_supplied_roots() {
        let Action::Dispatch {
            session,
            command,
            args: payload,
            ..
        } = parse(&args(&[
            "upload",
            "input[type=file]",
            "one.txt",
            "--session",
            "alpha",
            "two.txt",
        ]))
        .expect("upload dispatch")
        else {
            panic!("upload should dispatch");
        };
        assert_eq!(session, "alpha");
        assert_eq!(command, "upload");
        assert_eq!(
            payload,
            serde_json::json!({
                "selector":"input[type=file]",
                "files":["one.txt", "two.txt"]
            })
        );

        let Action::Dispatch { args: payload, .. } = parse(&args(&[
            "upload",
            "@e2",
            "--json",
            "--",
            "-leading-dash.txt",
        ]))
        .expect("upload paths after --") else {
            panic!("upload should dispatch");
        };
        assert_eq!(
            payload,
            serde_json::json!({
                "selector":"@e2",
                "files":["-leading-dash.txt"]
            })
        );
        assert!(parse(&args(&["upload"])).is_err());
        assert!(parse(&args(&["upload", "input[type=file]"])).is_err());
        assert!(parse(&args(&["upload", "--selector", "#file", "one.txt"])).is_err());
        assert!(
            parse(&args(&[
                "upload",
                "#file",
                "one.txt",
                "--allowed-dirs",
                "/tmp"
            ]))
            .is_err()
        );
    }

    #[test]
    fn cookies_cli_routes_are_scoped_and_values_are_masked_by_default() {
        let Action::Dispatch {
            command,
            args: payload,
            ..
        } = parse(&args(&["cookies", "list", "--reveal", "sid"])).unwrap()
        else {
            panic!("cookies list should dispatch");
        };
        assert_eq!(command, "cookies.list");
        assert_eq!(payload, serde_json::json!({"reveal":"sid"}));
        let Action::Dispatch {
            command,
            args: payload,
            ..
        } = parse(&args(&[
            "cookies",
            "clear",
            "sid",
            "--url",
            "https://example.test/path",
        ]))
        .unwrap()
        else {
            panic!("cookies clear should dispatch");
        };
        assert_eq!(command, "cookies.clear");
        assert_eq!(
            payload,
            serde_json::json!({"name":"sid","url":"https://example.test/path"})
        );
        assert!(parse(&args(&["cookies", "clear"])).is_err());
        assert!(parse(&args(&["cookies", "list", "extra"])).is_err());
        let Action::Dispatch {
            command,
            args: payload,
            ..
        } = parse(&args(&[
            "cookies",
            "set",
            "sid",
            "plain-value",
            "--url",
            "https://example.test/",
            "--domain",
            "example.test",
            "--path",
            "/account",
            "--secure",
            "--http-only",
        ]))
        .unwrap()
        else {
            panic!("cookies set should dispatch");
        };
        assert_eq!(command, "cookies.set");
        assert_eq!(
            payload,
            serde_json::json!({
                "cookie":{"name":"sid","value":"plain-value","domain":"example.test","path":"/account","expires":0.0,"size":0,"secure":true,"http_only":true,"session":false},
                "url":"https://example.test/"
            })
        );
        let Action::CookieImport { path, .. } =
            parse(&args(&["cookies", "set", "--curl", "jar.txt"])).unwrap()
        else {
            panic!("cookies curl import should dispatch");
        };
        assert_eq!(path, PathBuf::from("jar.txt"));
        assert_eq!(
            parse_curl_cookie_line(".example.test\tTRUE\t/\tTRUE\t0\tsid\tvalue").unwrap(),
            serde_json::json!({"name":"sid","value":"value","domain":".example.test","path":"/","expires":0.0,"size":0,"secure":true,"http_only":false,"session":false})
        );

        let data = serde_json::json!({"origin":"https://example.test","cookies":[
            {"name":"sid","value":"0123456789abcdef"},
            {"name":"short","value":"tiny"},
            {"name":"empty","value":""}
        ]});
        let masked = mask_cookie_list(data.clone(), "");
        assert_eq!(masked["cookies"][0]["value"], "0123••••cdef");
        assert_eq!(masked["cookies"][1]["value"], "••••");
        assert_eq!(masked["cookies"][2]["value"], "");
        assert_eq!(
            mask_cookie_list(data.clone(), " sid ")["cookies"][0]["value"],
            "0123456789abcdef"
        );
        assert_eq!(
            render_cookie_list_text(&data, "").unwrap(),
            "sid\t0123••••cdef\t\t\t-\nshort\t••••\t\t\t-\nempty\t\t\t\t-\n"
        );
        assert_eq!(
            render_cookie_list_json(&data, "", Vec::new()).unwrap(),
            "{\"success\":true,\"data\":{\"origin\":\"https://example.test\",\"cookies\":[{\"name\":\"sid\",\"value\":\"0123••••cdef\",\"domain\":\"\",\"path\":\"\",\"expires\":0,\"size\":0,\"http_only\":false,\"secure\":false,\"session\":false},{\"name\":\"short\",\"value\":\"••••\",\"domain\":\"\",\"path\":\"\",\"expires\":0,\"size\":0,\"http_only\":false,\"secure\":false,\"session\":false},{\"name\":\"empty\",\"value\":\"\",\"domain\":\"\",\"path\":\"\",\"expires\":0,\"size\":0,\"http_only\":false,\"secure\":false,\"session\":false}]}}\n"
        );
        let yaml_data = serde_json::json!({
            "origin": "https://fixture.invalid",
            "cookies": [{
                "name": "sid", "value": "0123••••cdef", "domain": "fixture.invalid",
                "path": "/", "expires": -1, "size": 20, "http_only": true,
                "secure": true, "session": true, "same_site": "Lax"
            }]
        });
        assert_eq!(
            render_cookie_list_yaml(&yaml_data, Vec::new()).unwrap(),
            "success: true\ndata:\n    origin: https://fixture.invalid\n    cookies:\n        - name: sid\n          value: 0123••••cdef\n          domain: fixture.invalid\n          path: /\n          expires: -1\n          size: 20\n          httponly: true\n          secure: true\n          session: true\n          samesite: Lax\nwarnings: []\nerror: null\n"
        );
    }

    #[test]
    fn runtime_event_commands_preserve_session_budget_and_format() {
        assert_eq!(
            parse(&args(&[
                "console",
                "list",
                "--session",
                "fixture",
                "--max-tokens",
                "12",
                "--json",
            ])),
            Ok(Action::RuntimeEvents {
                session: "fixture".to_owned(),
                command: "console.list".to_owned(),
                max_tokens: Some(12),
                format: Format::Json,
            })
        );
        assert_eq!(
            parse(&args(&["errors", "clear", "--session=fixture"])),
            Ok(Action::RuntimeEvents {
                session: "fixture".to_owned(),
                command: "errors.clear".to_owned(),
                max_tokens: None,
                format: Format::Text,
            })
        );
    }
}
