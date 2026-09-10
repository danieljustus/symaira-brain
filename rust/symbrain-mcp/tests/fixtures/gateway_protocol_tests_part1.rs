struct FixtureDispatcher;

const PROTOCOL_FIXTURE: &str = include_str!("gateway_protocol_cases.json");

#[test]
fn protocol_fixture_covers_required_loop_cases() {
    let fixture: Value = serde_json::from_str(PROTOCOL_FIXTURE).expect("protocol fixture");
    assert_eq!(fixture["schema_version"], 1);
    let cases = fixture["cases"].as_array().expect("fixture cases");
    for required in [
        "initialize",
        "tools-list",
        "tools-call",
        "initialized-notification",
        "cancelled-notification",
        "null-id",
        "framed-response",
        "malformed-line",
        "partial-body",
    ] {
        assert!(
            cases.iter().any(|case| case["id"] == required),
            "missing fixture case {required}"
        );
    }
}

impl Dispatcher for FixtureDispatcher {
    type Response = Response;

    fn dispatch(
        &self,
        request: &Request,
        _context: DispatchContext<'_>,
    ) -> Result<Option<Self::Response>, DispatchError> {
        let id = request.id.clone().unwrap_or(Value::Null);
        let response = match request.method.as_str() {
            "initialize" => Response::success(
                id,
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "fixture", "version": "0"}
                }),
            ),
            "ping" => Response::success(id, json!({})),
            "tools/list" => Response::success(
                id,
                json!({
                    "tools": [{
                        "name": "echo",
                        "description": "Echo input",
                        "inputSchema": {"type": "object"}
                    }]
                }),
            ),
            "tools/call" => {
                let params = request
                    .params
                    .as_deref()
                    .and_then(|raw| serde_json::from_str::<Value>(raw.get()).ok())
                    .ok_or_else(|| DispatchError::new("invalid call params"))?;
                if params.get("name").and_then(Value::as_str) == Some("echo") {
                    Response::success(
                        id,
                        json!({
                            "content": [{"type": "text", "text": "ok"}],
                            "isError": false
                        }),
                    )
                } else {
                    Response::error(id, CODE_INVALID_PARAMS, "Invalid params")
                }
            }
            _ => Response::error(id, -32601, "Method not found"),
        };
        Ok(Some(response))
    }

    fn error_response(&self, id: Value, code: i64, message: String) -> Self::Response {
        Response::error(id, code, message)
    }
}

fn framed(body: &str) -> String {
    format!("Content-Length: {}\r\n\r\n{body}", body.len())
}

#[test]
fn loop_dispatches_requests_and_keeps_transport_mode_symmetric() {
    let line = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
    let framed_body = r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#;
    let input = format!("{line}\n{}", framed(framed_body));
    let mut output = Vec::new();

    Server::new(FixtureDispatcher)
        .serve_io(input.as_bytes(), &mut output)
        .expect("serve");
    let text = String::from_utf8(output).expect("UTF-8 output");
    assert!(text.starts_with(
        r#"{"jsonrpc":"2.0","id":1,"result":{"capabilities":{"tools":{}},"protocolVersion":"2024-11-05","serverInfo":{"name":"fixture","version":"0"}}}"#
    ));
    assert!(text.contains("\nContent-Length: "));
    assert!(text.ends_with("\r\n\r\n{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}"));
}

#[test]
fn loop_dispatches_tools_list_and_call() {
    let input = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"echo","arguments":{}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"missing","arguments":{}}}"#,
        "\n"
    );
    let mut output = Vec::new();
    Server::new(FixtureDispatcher)
        .serve_io(input.as_bytes(), &mut output)
        .expect("serve");
    let responses: Vec<Value> = String::from_utf8(output)
        .expect("UTF-8 output")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON response"))
        .collect();
    assert_eq!(responses.len(), 3);
    let by_id = responses
        .into_iter()
        .map(|response| (response["id"].as_i64().expect("numeric id"), response))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(by_id[&1]["result"]["tools"][0]["name"], "echo");
    assert_eq!(by_id[&2]["result"]["isError"], false);
    assert_eq!(by_id[&3]["error"]["code"], CODE_INVALID_PARAMS);
}

#[test]
fn notifications_are_silent_but_null_id_is_a_request() {
    let input = concat!(
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":7}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#,
        "\n"
    );
    let mut output = Vec::new();
    Server::new(FixtureDispatcher)
        .serve_io(input.as_bytes(), &mut output)
        .expect("serve");
    assert_eq!(
        String::from_utf8(output).expect("UTF-8 output"),
        "{\"jsonrpc\":\"2.0\",\"id\":null,\"result\":{}}\n"
    );
}

#[test]
fn malformed_line_gets_parse_error_and_loop_continues() {
    let input = concat!(
        "{bad}\n",
        r#"{"jsonrpc":"2.0","id":4,"method":"ping"}"#,
        "\n"
    );
    let mut output = Vec::new();
    Server::new(FixtureDispatcher)
        .serve_io(input.as_bytes(), &mut output)
        .expect("serve");
    let text = String::from_utf8(output).expect("UTF-8 output");
    assert!(text.contains(&format!(r#""code":{CODE_PARSE_ERROR}"#)));
    assert!(text.contains(r#""id":4,"result":{}"#));
}

#[test]
fn malformed_framed_body_gets_framed_parse_error() {
    let input = b"Content-Length: 5\r\n\r\n{bad}";
    let mut output = Vec::new();
    Server::new(FixtureDispatcher)
        .serve_io(input.as_slice(), &mut output)
        .expect("serve");
    let output = String::from_utf8(output).expect("UTF-8 output");
    let (header, body) = output.split_once("\r\n\r\n").expect("framed error");
    let length = header
        .strip_prefix("Content-Length: ")
        .expect("content length")
        .parse::<usize>()
        .expect("content length integer");
    assert_eq!(length, body.len());
    let response: Value = serde_json::from_str(body).expect("JSON-RPC error");
    assert_eq!(response["id"], Value::Null);
    assert_eq!(response["error"]["code"], CODE_PARSE_ERROR);
}

#[test]
fn partial_framed_body_is_a_fatal_frame_error() {
    let input = b"Content-Length: 10\r\n\r\n{}";
    let result = Server::new(FixtureDispatcher).serve_io(input.as_slice(), &mut Vec::new());
    match result {
        Err(ServerError::Frame(FrameError::Io(error))) => {
            assert!(error.to_string().contains("read body: unexpected EOF"));
        }
        other => panic!("expected partial-frame error, got {other:?}"),
    }
}

struct FailingResponse;

impl Serialize for FailingResponse {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Err(S::Error::custom(
            "intentional response serialization failure",
        ))
    }
}

struct FailingDispatcher;

impl Dispatcher for FailingDispatcher {
    type Response = FailingResponse;

    fn dispatch(
        &self,
        _request: &Request,
        _context: DispatchContext<'_>,
    ) -> Result<Option<Self::Response>, DispatchError> {
        Ok(Some(FailingResponse))
    }

    fn error_response(&self, _id: Value, _code: i64, _message: String) -> Self::Response {
        FailingResponse
    }
}

#[test]
fn response_serialization_failure_is_returned() {
    let input = br#"{"jsonrpc":"2.0","id":1,"method":"ping"}
"#;
    let result = Server::new(FailingDispatcher).serve_io(input.as_slice(), &mut Vec::new());
    match result {
        Err(ServerError::Response(error)) => {
            assert!(
                error
                    .to_string()
                    .contains("intentional response serialization failure")
            );
        }
        other => panic!("expected visible response error, got {other:?}"),
    }
}

#[test]
fn concurrent_failures_preserve_dispatch_and_response_types() {
    let serialization_input = br#"{"jsonrpc":"2.0","id":1,"method":"tools/call"}
"#;
    let result =
        Server::new(FailingDispatcher).serve_io(serialization_input.as_slice(), &mut Vec::new());
    assert!(matches!(result, Err(ServerError::Response(_))));

    let dispatch_input = br#"{"jsonrpc":"2.0","id":2,"method":"tools/call"}
"#;
    let result =
        Server::new(FixtureDispatcher).serve_io(dispatch_input.as_slice(), &mut Vec::new());
    match result {
        Err(ServerError::Dispatch(error)) => {
            assert!(error.to_string().contains("invalid call params"));
        }
        other => panic!("expected typed dispatch error, got {other:?}"),
    }
}

struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("writer failed"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn writer_failure_is_returned() {
    let input = br#"{"jsonrpc":"2.0","id":1,"method":"ping"}
"#;
    let result = Server::new(FixtureDispatcher).serve_io(input.as_slice(), &mut FailingWriter);
    assert!(matches!(result, Err(ServerError::Response(_))));
}
