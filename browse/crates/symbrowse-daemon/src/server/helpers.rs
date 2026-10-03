use super::*;
pub(super) fn operation_result_for_frame(
    mut result: HandlerResult,
    frame: &Frame,
    operation: &OperationContext,
) -> Response {
    // Preserve the network refusal while adapting its public Go error shape.
    // Only literal private targets can be reclassified without another DNS
    // lookup; redirect/rebinding errors retain their original target details.
    if frame.cmd == "fetch.url"
        && let Err(error) = &mut result
        && error.code == codes::PEER_DENIED
        && error.message.starts_with("blocked_private:")
        && let Some(url) = frame
            .args
            .as_ref()
            .and_then(|args| args.get("url"))
            .and_then(Value::as_str)
        && let Ok(ip) = symbrowse_core::policy::policy_host(url).parse::<std::net::IpAddr>()
        && symbrowse_core::policy::is_private_ip(ip)
    {
        error.message = crate::redact_str(&format!(
            "blocked_private: {url} targets a private or loopback address"
        ));
        error.retryable = Some(false);
        error.requires_user_confirmation = Some(false);
        error.resume_hint = "the target is a private or loopback address; start the daemon with --allow-private to permit it".into();
    }
    operation_result_response(result, operation)
}

pub(super) fn operation_result_response(
    result: HandlerResult,
    operation: &OperationContext,
) -> Response {
    if operation.remaining().is_zero() {
        return error_response(
            codes::OPERATION_TIMEOUT,
            "daemon operation exceeded its timeout",
        );
    }
    match result {
        Ok((data, warnings)) => success_response(data, warnings),
        Err(error) => Response {
            success: false,
            data: None,
            error: Some(crate::redaction::redact_error(
                if error.code == codes::UNKNOWN_COMMAND {
                    DaemonError {
                        code: error.code,
                        message: "command is not implemented by the daemon".into(),
                        ..Default::default()
                    }
                } else {
                    error
                },
            )),
            warnings: Vec::new(),
        },
    }
}

pub(super) fn session_error_response(error: crate::SessionError) -> Response {
    let (code, message) = match error {
        crate::SessionError::InvalidName(message) => (codes::INVALID_SESSION, message),
        crate::SessionError::NotFound(message) => (codes::SESSION_NOT_FOUND, message),
        crate::SessionError::Io(message) | crate::SessionError::InvalidValue(message) => {
            (codes::OPERATION_FAILED, message)
        }
    };
    error_response(code, message)
}

pub(super) fn unix_nanos() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos().min(i64::MAX as u128) as i64)
}
pub(super) fn elapsed_since(start: i64) -> Duration {
    Duration::from_nanos(unix_nanos().saturating_sub(start) as u64)
}
pub(super) fn format_time(nanos: i64) -> String {
    let Ok(value) = time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(nanos)) else {
        return "1970-01-01T00:00:00Z".into();
    };
    value
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

pub(super) fn default_user_data_root() -> PathBuf {
    if let Ok(path) = std::env::var("SYMBROWSE_USER_DATA_DIR") {
        return PathBuf::from(path);
    }
    if cfg!(target_os = "macos")
        && let Ok(home) = std::env::var("HOME")
    {
        return PathBuf::from(home).join("Library/Caches/symbrowse/sessions");
    }
    if cfg!(windows)
        && let Ok(local_app_data) = std::env::var("LOCALAPPDATA")
    {
        return PathBuf::from(local_app_data).join("symbrowse/sessions");
    }
    if let Ok(path) = std::env::var("XDG_CACHE_HOME") {
        return PathBuf::from(path).join("symbrowse/sessions");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".cache/symbrowse/sessions");
    }
    std::env::temp_dir().join("symbrowse/sessions")
}

pub fn validate_session(session: &str) -> bool {
    !session.is_empty()
        && session.len() <= 64
        && session
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

pub fn socket_path(base: impl AsRef<Path>, session: &str) -> Result<PathBuf, ServerError> {
    if !validate_session(session) {
        return Err(ServerError::InvalidSession(session.into()));
    }
    Ok(base.as_ref().join(format!("{session}.sock")))
}

pub fn default_socket_path(session: &str) -> PathBuf {
    crate::spec::default_socket_path(session)
}

#[cfg(not(unix))]
pub(super) fn builtin_handler(_frame: Frame) -> HandlerResult {
    Err(DaemonError {
        code: codes::OPERATION_FAILED.into(),
        message: "daemon state operations are unavailable on this platform".into(),
        ..Default::default()
    })
}

#[cfg(unix)]
pub(super) fn builtin_handler(frame: Frame) -> HandlerResult {
    if !frame.cmd.starts_with("state.") {
        return Err(DaemonError {
            code: codes::UNKNOWN_COMMAND.into(),
            message: "command is not implemented by the daemon".into(),
            ..Default::default()
        });
    }
    if matches!(frame.cmd.as_str(), "state.save" | "state.load") {
        return Err(DaemonError {
            code: codes::OPERATION_FAILED.into(),
            message: "browser-backed state capture and restore are not implemented in Rust".into(),
            hint: "use the Go fallback until the browser state runtime passes parity".into(),
            ..Default::default()
        });
    }
    let store = Store::new(state_root(), time::Duration::days(30), None).map_err(store_error)?;
    let args = frame.args.as_ref().and_then(Value::as_object);
    match frame.cmd.as_str() {
        "state.list" => Ok((
            Some(json!({"schema_version": 1, "states": store.list().map_err(store_error)?})),
            Vec::new(),
        )),

        "state.show" => {
            let name = required_state_name(args)?;
            let metadata = store.metadata(name).map_err(store_error)?;
            Ok((
                Some(serde_json::to_value(metadata).map_err(|e| DaemonError {
                    code: codes::OPERATION_FAILED.into(),
                    message: e.to_string(),
                    ..Default::default()
                })?),
                Vec::new(),
            ))
        }
        "state.clear" => {
            let name = required_state_name(args)?;
            store.remove(name).map_err(store_error)?;
            Ok((Some(json!({"name": name, "cleared": true})), Vec::new()))
        }
        "state.clean" => {
            let removed = if let Some(days) = args
                .and_then(|value| value.get("older_than_days"))
                .and_then(Value::as_i64)
            {
                store
                    .clean_older_than_at(
                        time::Duration::days(days),
                        time::OffsetDateTime::now_utc(),
                    )
                    .map_err(store_error)?
            } else {
                store
                    .clean_at(time::OffsetDateTime::now_utc())
                    .map_err(store_error)?
            };
            Ok((Some(json!({"removed": removed})), Vec::new()))
        }
        _ => Err(DaemonError {
            code: codes::UNKNOWN_COMMAND.into(),
            message: "command is not implemented by the daemon".into(),
            ..Default::default()
        }),
    }
}

#[cfg(unix)]
pub(super) fn required_state_name(
    args: Option<&serde_json::Map<String, Value>>,
) -> Result<&str, DaemonError> {
    args.and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| DaemonError {
            code: codes::MALFORMED_REQUEST.into(),
            message: "state name is required".into(),
            ..Default::default()
        })
}
#[cfg(unix)]
pub(super) fn store_error(error: impl std::fmt::Display) -> DaemonError {
    DaemonError {
        code: codes::OPERATION_FAILED.into(),
        message: crate::redact_str(&error.to_string()),
        ..Default::default()
    }
}
#[cfg(unix)]
pub(super) fn state_root() -> PathBuf {
    if let Ok(path) = std::env::var("SYMBROWSE_STATE_DIR") {
        return PathBuf::from(path).join("states");
    }
    if let Ok(path) = std::env::var("XDG_STATE_HOME") {
        return PathBuf::from(path).join("symbrowse/states");
    }
    std::env::var("HOME").map_or_else(
        |_| std::env::temp_dir().join("symbrowse/states"),
        |home| PathBuf::from(home).join(".local/state/symbrowse/states"),
    )
}
