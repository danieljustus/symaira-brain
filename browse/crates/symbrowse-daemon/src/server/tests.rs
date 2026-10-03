use super::*;

#[test]
fn status_timestamps_use_rfc3339_nano() {
    assert_eq!(format_time(0), "1970-01-01T00:00:00Z");
    assert_eq!(format_time(1_234_000_000), "1970-01-01T00:00:01.234Z");
}
#[test]
fn sessions_are_path_safe() {
    assert!(validate_session("default-1"));
    assert!(!validate_session("../escape"));
    assert!(!validate_session(""));
}

#[test]
fn operation_context_is_cancelled_after_its_deadline() {
    let operation = OperationContext {
        cancelled: Arc::new(AtomicBool::new(false)),
        shutdown: Arc::new(AtomicBool::new(false)),
        deadline: Instant::now() - Duration::from_millis(1),
    };
    assert!(operation.remaining().is_zero());
    assert!(operation.is_cancelled());
    let response =
        operation_result_response(Ok((Some(json!({"done": true})), Vec::new())), &operation);
    assert!(!response.success);
    let error = response.error.expect("expired result must be a timeout");
    assert_eq!(error.code, codes::OPERATION_TIMEOUT);
    assert_eq!(error.message, "daemon operation exceeded its timeout");
}

#[test]
fn paths_use_one_session_component() {
    assert_eq!(
        socket_path("/tmp/run", "x").unwrap(),
        PathBuf::from("/tmp/run/x.sock")
    );
}

#[cfg(windows)]
#[test]
fn first_pipe_instance_contention_is_already_running() {
    let error = io::Error::from_raw_os_error(5); // ERROR_ACCESS_DENIED
    assert!(matches!(
        named_pipe_create_error(error),
        ServerError::AlreadyRunning
    ));
}

#[cfg(unix)]
#[test]
fn browser_state_save_and_load_fail_before_store_access() {
    for command in ["state.save", "state.load"] {
        let error = builtin_handler(Frame {
            cmd: command.to_owned(),
            args: Some(json!({"name": "never-written"})),
            ..Frame::default()
        })
        .expect_err("browser-backed state operations must be rejected");
        assert_eq!(error.code, codes::OPERATION_FAILED);
        assert!(error.message.contains("not implemented"));
    }
}
