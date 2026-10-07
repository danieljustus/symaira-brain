use super::*;

pub(super) fn should_autostart(error: &ClientError) -> bool {
    matches!(
        error,
        ClientError::Transport(DaemonError {
            code,
            ..
        }) if code == codes::DAEMON_UNAVAILABLE
    )
}

pub(super) fn map_io_error(
    options: &ClientOptions,
    error: io::Error,
    operation: &str,
) -> ClientError {
    if matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ) {
        return ClientError::Transport(DaemonError {
            code: codes::OPERATION_TIMEOUT.into(),
            message: format!("daemon response timed out after {:?}", options.read_timeout),
            hint: format!(
                "increase timeout with SYMBROWSE_READ_TIMEOUT or inspect daemon logs for session {:?}",
                options.session
            ),
            details: Some(redact_json(&serde_json::json!({
                "session": options.session, "socket_path": options.socket_path,
                "timeout_seconds": options.read_timeout.as_secs_f64(),
            }))),
            ..Default::default()
        });
    }
    let context = if error.kind() == io::ErrorKind::UnexpectedEof {
        "daemon closed connection without a response"
    } else if operation.contains("write") || operation.contains("flush") {
        "failed to write daemon request"
    } else if operation.contains("deadline") {
        "failed to set daemon deadline"
    } else {
        "failed to read daemon response"
    };
    lifecycle_error(
        options,
        format!("{context} for session {:?}", options.session),
    )
}

pub(super) fn unavailable(options: &ClientOptions, _error: io::Error) -> ClientError {
    lifecycle_error(
        options,
        format!("daemon is unavailable for session {:?}", options.session),
    )
}

pub(super) fn lifecycle_error(options: &ClientOptions, message: String) -> ClientError {
    let disabled =
        !options.autostart || std::env::var("SYMBROWSE_NO_AUTOSTART").as_deref() == Ok("1");
    let log = options
        .start
        .as_ref()
        .map_or_else(default_log_path, |start| start.log_path.clone());
    ClientError::Transport(DaemonError {
        code: codes::DAEMON_UNAVAILABLE.into(),
        message: redact_str(&message),
        hint: redact_str(&format!(
            "start daemon with 'symbrowse daemon --session {}'{}; see daemon log at {}",
            options.session,
            if disabled {
                " (autostart disabled via SYMBROWSE_NO_AUTOSTART)"
            } else {
                ""
            },
            log.display()
        )),
        details: Some(redact_json(
            &serde_json::json!({"session": options.session, "socket_path": options.socket_path}),
        )),
        ..Default::default()
    })
}
