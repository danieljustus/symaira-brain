use super::*;
#[allow(clippy::too_many_arguments)]
pub(super) fn serve_connection_parts<S>(
    stream: S,
    handler: DaemonHandler,
    options: ServerOptions,
    last_activity: Arc<AtomicI64>,
    stopping: Arc<AtomicBool>,
    started_at: i64,
    registry: Arc<crate::SessionRegistry>,
    dispatch_gate: Arc<std::sync::Mutex<()>>,
) where
    S: io::Read + Write + Send + 'static,
{
    let mut reader = BufReader::new(stream);
    loop {
        // Do not start another read after shutdown has won. On Windows a
        // named-pipe read can outlive the listener wake-up even though the
        // worker is otherwise cancellation-aware.
        if stopping.load(Ordering::Acquire) {
            return;
        }
        let mut line = Vec::new();
        #[cfg(windows)]
        let read_result = read_limited_line_windows(
            &mut reader,
            &mut line,
            MAX_FRAME_BYTES,
            options.read_timeout,
            &stopping,
        );
        #[cfg(not(windows))]
        let read_result = read_limited_line(&mut reader, &mut line, MAX_FRAME_BYTES);
        match read_result {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        last_activity.store(unix_nanos(), Ordering::Release);
        let frame = match decode_frame(line.trim_ascii_end()) {
            Ok(mut frame) => {
                if frame.session.is_empty() {
                    frame.session = options.session.clone();
                }
                frame
            }
            Err(error) => {
                if write_response(reader.get_mut(), error_response(error.code, error.message))
                    .is_err()
                {
                    return;
                }
                continue;
            }
        };
        // The gate covers every registry mutation and the dispatch decision.
        // stop() takes the same gate, so no request can start after shutdown
        // has won the transition.
        let _dispatch = match dispatch_gate.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        if stopping.load(Ordering::Acquire) {
            return;
        }
        if !validate_session(&frame.session) {
            if write_response(
                reader.get_mut(),
                error_response(
                    codes::INVALID_SESSION,
                    format!("invalid session {}", crate::redact_str(&frame.session)),
                ),
            )
            .is_err()
            {
                return;
            }
            continue;
        }
        if frame.cmd == "session.list" {
            if write_response(
                reader.get_mut(),
                success_response(
                    Some(serde_json::to_value(registry.list_data()).unwrap_or(Value::Null)),
                    Vec::new(),
                ),
            )
            .is_err()
            {
                return;
            }
            continue;
        }
        if frame.cmd == "session.info" {
            let response = match registry.touch(&frame.session) {
                Ok(()) => match registry.get(&frame.session) {
                    Ok(info) => success_response(
                        Some(serde_json::to_value(info).unwrap_or(Value::Null)),
                        Vec::new(),
                    ),
                    Err(error) => session_error_response(error),
                },
                Err(error) => session_error_response(error),
            };
            if write_response(reader.get_mut(), response).is_err() {
                return;
            }
            continue;
        }
        if let Err(error) = registry.ensure(&frame.session) {
            if write_response(reader.get_mut(), session_error_response(error)).is_err() {
                return;
            }
            continue;
        }
        let _ = registry.touch(&frame.session);
        let cmd = frame.cmd.clone();
        if cmd == "daemon.status" {
            let data = json!({"running":true,"pid":std::process::id(),"session":options.session,"socket":crate::redact_str(&options.socket_path.to_string_lossy()),"started_at":format_time(started_at),"last_activity":format_time(last_activity.load(Ordering::Acquire)),"policy":options.policy,"engine":options.engine,"mode":options.mode});
            if write_response(reader.get_mut(), success_response(Some(data), Vec::new())).is_err() {
                return;
            }
            continue;
        }
        if cmd == "daemon.stop" {
            if write_response(
                reader.get_mut(),
                success_response(Some(json!({"stopping":true})), Vec::new()),
            )
            .is_err()
            {
                return;
            }
            stopping.store(true, Ordering::Release);
            #[cfg(windows)]
            wake_windows_listener(&options.socket_path);
            return;
        }
        if cmd == "daemon.ping" {
            if write_response(
                reader.get_mut(),
                success_response(Some(json!({"pong":true})), Vec::new()),
            )
            .is_err()
            {
                return;
            }
            continue;
        }
        drop(_dispatch);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let handler_clone = handler.clone();
        let frame_clone = frame.clone();
        let timeout = options.operation_timeout;
        let operation = OperationContext {
            cancelled: Arc::new(AtomicBool::new(false)),
            shutdown: stopping.clone(),
            deadline: Instant::now() + timeout,
        };
        let handler_operation = operation.clone();
        thread::spawn(move || {
            let _ = tx.send(handler_clone(frame_clone, handler_operation));
        });
        let result = loop {
            match rx.recv_timeout(operation.remaining().min(Duration::from_millis(10))) {
                Ok(result) => break operation_result_response(result, &operation),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    break error_response(codes::OPERATION_FAILED, "daemon handler disconnected");
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    if stopping.load(Ordering::Acquire) =>
                {
                    // Shutdown is cooperative: notify the handler through its
                    // context and give it a bounded cancellation window. Do
                    // not wait for the full request deadline during shutdown.
                    operation.cancel();
                    let shutdown_deadline = Instant::now() + Duration::from_millis(100);
                    match rx
                        .recv_timeout(shutdown_deadline.saturating_duration_since(Instant::now()))
                    {
                        Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return,
                    }
                    return;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                    if operation.remaining().is_zero() =>
                {
                    operation.cancel();
                    break error_response(
                        codes::OPERATION_TIMEOUT,
                        "daemon operation exceeded its timeout",
                    );
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            }
        };
        if write_response(reader.get_mut(), result).is_err() {
            return;
        }
    }
}

pub(super) fn write_response<W: Write>(stream: &mut W, response: Response) -> io::Result<()> {
    let mut data = serde_json::to_vec(&response).map_err(io::Error::other)?;
    if data.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "daemon response exceeds size limit",
        ));
    }
    data.push(b'\n');
    stream.write_all(&data)?;
    stream.flush()
}

#[cfg(not(windows))]
pub(super) fn read_limited_line<R: BufRead>(
    reader: &mut R,
    output: &mut Vec<u8>,
    limit: usize,
) -> io::Result<usize> {
    output.clear();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(output.len());
        }
        let take = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |n| n + 1);
        if output.len().saturating_add(take) > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "daemon frame exceeds size limit",
            ));
        }
        output.extend_from_slice(&available[..take]);
        reader.consume(take);
        if output.last() == Some(&b'\n') {
            return Ok(output.len());
        }
    }
}
