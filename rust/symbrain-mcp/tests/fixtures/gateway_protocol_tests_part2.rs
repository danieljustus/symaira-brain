struct ConcurrentDispatcher {
    active: AtomicUsize,
    overlapped: AtomicBool,
}

impl Dispatcher for ConcurrentDispatcher {
    type Response = Response;

    fn dispatch(
        &self,
        request: &Request,
        _context: DispatchContext<'_>,
    ) -> Result<Option<Self::Response>, DispatchError> {
        if request.method == "tools/call" {
            if self.active.fetch_add(1, Ordering::AcqRel) > 0 {
                self.overlapped.store(true, Ordering::Release);
            }
            thread::sleep(Duration::from_millis(20));
            self.active.fetch_sub(1, Ordering::AcqRel);
        }
        Ok(Some(Response::success(
            request.id.clone().unwrap_or(Value::Null),
            json!({}),
        )))
    }

    fn error_response(&self, id: Value, code: i64, message: String) -> Self::Response {
        Response::error(id, code, message)
    }
}

#[test]
fn tool_calls_dispatch_concurrently_and_join_before_eof() {
    let dispatcher = ConcurrentDispatcher {
        active: AtomicUsize::new(0),
        overlapped: AtomicBool::new(false),
    };
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call"}"#,
        "\n"
    );
    let mut output = Vec::new();
    Server::new(&dispatcher)
        .serve_io(input.as_bytes(), &mut output)
        .expect("serve");
    assert!(dispatcher.overlapped.load(Ordering::Acquire));
    assert_eq!(String::from_utf8(output).unwrap().lines().count(), 2);
}

struct CancellationDispatcher {
    started: mpsc::Sender<()>,
}

impl Dispatcher for CancellationDispatcher {
    type Response = Response;

    fn dispatch(
        &self,
        request: &Request,
        context: DispatchContext<'_>,
    ) -> Result<Option<Self::Response>, DispatchError> {
        if request.method == "tools/call" {
            self.started.send(()).expect("started receiver");
            while !context.is_cancelled() {
                thread::yield_now();
            }
        }
        Ok(Some(Response::success(
            request.id.clone().unwrap_or(Value::Null),
            json!({}),
        )))
    }

    fn error_response(&self, id: Value, code: i64, message: String) -> Self::Response {
        Response::error(id, code, message)
    }
}

#[test]
fn cancellation_context_reaches_in_flight_tool_dispatch() {
    let (started_tx, started_rx) = mpsc::channel();
    let dispatcher = CancellationDispatcher {
        started: started_tx,
    };
    let cancelled = std::sync::Arc::new(AtomicBool::new(false));
    let cancellation = std::sync::Arc::clone(&cancelled);
    let input = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call"}
"#;
    let server_thread = thread::spawn(move || {
        let mut output = Vec::new();
        let result = Server::new(&dispatcher).serve_io_with_cancel(
            cancellation.as_ref(),
            input.as_bytes(),
            &mut output,
        );
        (result, output)
    });
    started_rx.recv().expect("handler started");
    cancelled.store(true, Ordering::Release);
    let (result, output) = server_thread.join().expect("server thread");
    assert!(matches!(result, Err(ServerError::Cancelled)));
    assert!(serde_json::from_slice::<Value>(&output).is_ok());
}

struct PanicDispatcher;

impl Dispatcher for PanicDispatcher {
    type Response = Response;

    fn dispatch(
        &self,
        _request: &Request,
        _context: DispatchContext<'_>,
    ) -> Result<Option<Self::Response>, DispatchError> {
        panic!("test dispatcher panic");
    }

    fn error_response(&self, id: Value, code: i64, message: String) -> Self::Response {
        Response::error(id, code, message)
    }
}

#[test]
fn dispatcher_panics_become_internal_errors_in_sync_and_concurrent_modes() {
    let mut line_output = Vec::new();
    Server::new(PanicDispatcher)
        .serve_io(
            r#"{"jsonrpc":"2.0","id":1,"method":"ping"}
"#
            .as_bytes(),
            &mut line_output,
        )
        .expect("sync panic is contained");
    let line: Value = serde_json::from_slice(&line_output).expect("sync response");
    assert_eq!(line["error"]["code"], CODE_INTERNAL_ERROR);
    assert_eq!(line["error"]["message"], "Internal error: handler panicked");

    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call"}"#,
        "\n"
    );
    let mut concurrent_output = Vec::new();
    Server::new(PanicDispatcher)
        .serve_io(input.as_bytes(), &mut concurrent_output)
        .expect("concurrent panic is contained");
    let responses: Vec<Value> = String::from_utf8(concurrent_output)
        .expect("UTF-8 responses")
        .lines()
        .map(|line| serde_json::from_str(line).expect("concurrent response"))
        .collect();
    assert_eq!(responses.len(), 2);
    for response in responses {
        assert_eq!(response["error"]["code"], CODE_INTERNAL_ERROR);
        assert_eq!(
            response["error"]["message"],
            "Internal error: handler panicked"
        );
    }
}

#[test]
fn every_write_mode_flushes_buffered_writer() {
    let response = Response::success(Value::from(1), json!({}));
    for mode in [symbrain_mcp::Mode::Line, symbrain_mcp::Mode::Framed] {
        let mut output = Vec::new();
        let mut writer = BufWriter::new(&mut output);
        write_message(&mut writer, mode, &response).expect("write response");
        assert!(
            writer.buffer().is_empty(),
            "{mode:?} response was not flushed"
        );
        drop(writer);
        assert!(!output.is_empty());
    }
}

#[test]
fn excessive_framed_headers_are_rejected_by_total_bytes_and_line_count() {
    let mut bytes = "Content-Length: 2\r\n".to_string();
    bytes.push_str("X: ");
    bytes.push_str(&"a".repeat(symbrain_mcp::MAX_HEADER_BYTES / 2));
    bytes.push_str("\r\nX: ");
    bytes.push_str(&"a".repeat(symbrain_mcp::MAX_HEADER_BYTES / 2));
    bytes.push_str("\r\n\r\n{}");
    let mut decoder = symbrain_mcp::Decoder::new(std::io::Cursor::new(bytes));
    assert!(matches!(
        decoder.read_request(),
        Err(FrameError::HeaderTooLong)
    ));

    let mut lines = "Content-Length: 2\r\n".to_string();
    for _ in 0..symbrain_mcp::MAX_HEADER_LINES {
        lines.push_str("X: y\r\n");
    }
    lines.push_str("\r\n{}");
    let mut decoder = symbrain_mcp::Decoder::new(std::io::Cursor::new(lines));
    assert!(matches!(
        decoder.read_request(),
        Err(FrameError::TooManyHeaders)
    ));
}

#[test]
fn invalid_jsonrpc_envelopes_receive_invalid_request_errors() {
    let inputs = [
        "{}".to_string(),
        "null".to_string(),
        serde_json::json!({"jsonrpc": "1.0", "method": "ping"}).to_string(),
        serde_json::json!({"jsonrpc": "2.0"}).to_string(),
        serde_json::json!({"jsonrpc": "2.0", "method": ""}).to_string(),
    ];
    for input in inputs {
        let mut output = Vec::new();
        Server::new(FixtureDispatcher)
            .serve_io(format!("{input}\n").as_bytes(), &mut output)
            .expect("serve invalid request");
        let response: Value = serde_json::from_slice(&output).expect("invalid request response");
        assert_eq!(response["id"], Value::Null);
        assert_eq!(
            response["error"]["code"],
            symbrain_mcp::CODE_INVALID_REQUEST
        );
    }
}
