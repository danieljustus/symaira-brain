//! Read-only daemon/session inspection shares the unified CLI error contract.

use super::{Action, ParseError, parse_format, required_value, write_stdout};
use std::{
    io::{self, Write},
    process::ExitCode,
};
use symbrowse_core::{
    error::ErrorCode,
    output::{Envelope, ErrorPayload, Format, Warning},
};
use symbrowse_daemon::{Client, ClientError, ClientOptions, Frame, default_socket_path};

pub(super) fn run_daemon_lifecycle(session: String, command: String, format: Format) -> ExitCode {
    if !symbrowse_daemon::validate_session(&session) {
        return render_result(
            Err(ClientError::Io(io::Error::other(format!(
                "invalid session {session:?}"
            )))),
            format,
        );
    }
    let client = Client::new(ClientOptions {
        socket_path: default_socket_path(&session),
        session: session.clone(),
        ..Default::default()
    });
    render_result(
        client.request_without_autostart(Frame {
            cmd: command,
            session,
            ..Default::default()
        }),
        format,
    )
}

pub(super) fn render_result(
    result: Result<symbrowse_daemon::Response, ClientError>,
    format: Format,
) -> ExitCode {
    let response = match result {
        Ok(mut response) => {
            // Go wraps a protocol failure as a CLI error with the protocol
            // error as its cause. Preserve its established CLI message.
            if let Some(error) = response.error.as_mut() {
                error.message = format!("{}: {}", error.message, error.message);
            }
            response
        }
        Err(ClientError::Transport(error)) => symbrowse_daemon::Response {
            error: Some(error),
            ..Default::default()
        },
        Err(error) => symbrowse_daemon::error_response("internal", error.to_string()),
    };
    let envelope = Envelope {
        success: response.success,
        data: response.data.unwrap_or(serde_json::Value::Null),
        warnings: response
            .warnings
            .into_iter()
            .map(|w| Warning {
                kind: w.kind,
                severity: w.severity,
                message: w.message,
                r#ref: w.r#ref,
                excerpt: w.excerpt,
            })
            .collect(),
        error: response.error.map(|error| ErrorPayload {
            code: serde_json::from_value(serde_json::Value::String(error.code))
                .unwrap_or(ErrorCode::Internal),
            message: error.message,
            hint: error.hint,
            details: error.details,
            retryable: error.retryable,
            requires_user_confirmation: error.requires_user_confirmation,
            resume_hint: error.resume_hint,
        }),
    };
    let status = envelope
        .error
        .as_ref()
        .map_or(0, |error| error.code.exit_code());
    match envelope.render(format) {
        Ok(_) if !envelope.success && format == Format::Text => {
            let message = envelope
                .error
                .as_ref()
                .map(|e| e.message.as_str())
                .unwrap_or("daemon request failed");
            let result = writeln!(io::stderr(), "{message}");
            if result.is_err() {
                return ExitCode::from(1);
            }
        }
        Ok(output) => {
            if write_stdout(&output) != ExitCode::SUCCESS {
                return ExitCode::from(1);
            }
        }
        Err(_) => return ExitCode::from(1),
    }
    ExitCode::from(status)
}

pub(super) fn parse_session(values: &[String], command_index: usize) -> Result<Action, ParseError> {
    let mut session = "default".to_owned();
    let mut subcommand = None;
    let mut format = Format::Text;
    let mut json = false;
    let mut index = 0;
    while index < values.len() {
        match values[index].as_str() {
            _ if index == command_index => {}
            "--session" => {
                index += 1;
                session = required_value(values, index, "--session")?.to_owned();
            }
            value if value.starts_with("--session=") => session = value[10..].to_owned(),
            "--json" | "--json=true" => json = true,
            "--json=false" => json = false,
            "--output" => {
                index += 1;
                format = parse_format(required_value(values, index, "--output")?)?;
            }
            value if value.starts_with("--output=") => format = parse_format(&value[9..])?,
            "list" | "info" if index > command_index && subcommand.is_none() => {
                subcommand = Some(values[index].clone());
            }
            value => {
                return Err(ParseError {
                    message: format!("unknown argument {value:?} for session"),
                    exit_code: 2,
                });
            }
        }
        index += 1;
    }
    let Some(subcommand) = subcommand else {
        return Ok(Action::Help(help(None)));
    };
    Ok(Action::DaemonLifecycle {
        session,
        command: format!("session.{subcommand}"),
        format: if json { Format::Json } else { format },
    })
}

pub(super) fn help(subcommand: Option<&str>) -> String {
    match subcommand {
        Some(command @ ("list" | "info")) => format!("{}\n\nUsage:\n  symbrowse session {command} [flags]\n\nFlags:\n  -h, --help   help for {command}\n\nGlobal Flags:\n      --json             print the unified machine-readable output envelope (shorthand for --output json)\n      --output string    output format: text, json or yaml (--json is shorthand for --output json) (default \"text\")\n      --session string   session name (default \"default\")\n", if command == "list" { "List daemon-owned sessions" } else { "Show session information" }),
        _ => "Inspect daemon-owned sessions\n\nUsage:\n  symbrowse session [command]\n\nAvailable Commands:\n  info        Show session information\n  list        List daemon-owned sessions\n\nFlags:\n  -h, --help             help for session\n      --session string   session name (default \"default\")\n".into(),
    }
}

pub(super) fn run_state_operation(
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
    let mut response = match client.request(Frame {
        cmd: command.clone(),
        args,
        session,
        ..Frame::default()
    }) {
        Ok(response) => response,
        Err(error) => return render_result(Err(error), format),
    };
    if format == Format::Yaml
        && command == "state.clean"
        && let Some(data) = response
            .data
            .as_mut()
            .and_then(serde_json::Value::as_object_mut)
        && data.get("removed") == Some(&serde_json::Value::Null)
    {
        // Go's typed clean CLI payload renders a nil slice as [] in
        // YAML; the daemon/JSON wire payload remains null.
        data.insert("removed".into(), serde_json::json!([]));
    }
    if !response.success || format != Format::Text {
        return render_result(Ok(response), format);
    }
    let data = response.data.unwrap_or(serde_json::Value::Null);
    let output = match command.as_str() {
        "state.save" => format!(
            "saved state {:?}\n",
            data.get("saved")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
        ),
        "state.load" => format!(
            "loaded state {:?}\n",
            data.get("loaded")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
        ),
        "state.clear" => format!(
            "cleared state {:?}\n",
            data.get("cleared")
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_selection_and_format_are_independent_of_flag_position() {
        for arguments in [
            vec!["--json", "session", "list", "--session", "alpha"],
            vec![
                "session",
                "--session=alpha",
                "list",
                "--json",
                "--output=yaml",
            ],
        ] {
            let values: Vec<_> = arguments.iter().map(|v| (*v).to_owned()).collect();
            let command_index = values.iter().position(|v| v == "session").unwrap();
            assert_eq!(
                parse_session(&values, command_index).unwrap(),
                Action::DaemonLifecycle {
                    session: "alpha".into(),
                    command: "session.list".into(),
                    format: Format::Json,
                }
            );
        }
    }
}
