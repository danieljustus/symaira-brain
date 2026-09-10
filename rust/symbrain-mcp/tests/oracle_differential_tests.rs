use std::io::Cursor;

use serde::Deserialize;
use serde_json::{Value, json};
use symbrain_mcp::{CODE_PARSE_ERROR, Decoder, MAX_MESSAGE_BYTES, Mode, Response, write_message};

#[derive(Deserialize)]
struct Suite {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    #[serde(default)]
    input: String,
    #[serde(default)]
    generator: String,
    #[serde(default)]
    output: String,
    #[serde(default)]
    error_kind: String,
    #[serde(default)]
    error: String,
}

fn input(case: &Case) -> Vec<u8> {
    const PING: &str = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
    match case.generator.as_str() {
        "oversized_line" => {
            let mut input = Vec::with_capacity(MAX_MESSAGE_BYTES + 2);
            input.push(b'{');
            input.resize(MAX_MESSAGE_BYTES + 1, b' ');
            input.push(b'\n');
            input
        }
        "max_framed" => {
            let mut body = PING.as_bytes().to_vec();
            body.resize(MAX_MESSAGE_BYTES, b' ');
            let mut input = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
            input.extend(body);
            input
        }
        _ => case.input.as_bytes().to_vec(),
    }
}

fn response_payload(output: &str) -> &str {
    output
        .split_once("\r\n\r\n")
        .map_or_else(|| output.trim(), |(_, body)| body)
}

#[test]
fn framing_and_response_bytes_match_go_oracle() {
    let suite: Suite = serde_json::from_slice(include_bytes!("fixtures/oracle_expectations.json"))
        .expect("parse MCP oracle");
    for case in suite.cases {
        let mut decoder = Decoder::new(Cursor::new(input(&case)));
        match case.error_kind.as_str() {
            "read" => {
                let error = decoder
                    .read_request()
                    .expect_err(&format!("case {} should fail framing", case.id));
                assert!(!error.is_parse(), "case {} returned parse error", case.id);
                assert_eq!(error.to_string(), case.error, "case {} error", case.id);
            }
            "parse" => {
                let error = decoder
                    .read_request()
                    .expect_err(&format!("case {} should fail parsing", case.id));
                assert!(error.is_parse(), "case {} returned framing error", case.id);
                let output: Value = serde_json::from_str(response_payload(&case.output))
                    .expect("parse Go error response");
                assert_eq!(output["id"], Value::Null);
                assert_eq!(output["error"]["code"], CODE_PARSE_ERROR);
                let expected_mode = if case.output.starts_with("Content-Length:") {
                    Mode::Framed
                } else {
                    Mode::Line
                };
                assert_eq!(error.mode(), expected_mode, "case {} mode", case.id);
            }
            "" => {
                let (request, mode) = decoder
                    .read_request()
                    .unwrap_or_else(|error| panic!("case {} failed: {error}", case.id))
                    .unwrap_or_else(|| panic!("case {} reached EOF", case.id));
                if request.is_notification() {
                    assert!(
                        case.output.is_empty(),
                        "case {} notification output",
                        case.id
                    );
                    continue;
                }
                let response = Response::success(request.id.unwrap_or(Value::Null), json!({}));
                let mut output = Vec::new();
                write_message(&mut output, mode, &response).expect("write response");
                assert_eq!(output, case.output.as_bytes(), "case {} output", case.id);
            }
            other => panic!("case {} has unknown error kind {other}", case.id),
        }
    }
}

#[test]
fn params_preserve_raw_json_bytes() {
    let body = br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params": { "name" : "x" }}"#;
    let mut decoder = Decoder::new(Cursor::new(body));
    let request = decoder.read_request().unwrap().unwrap().0;
    assert_eq!(request.params.expect("params").get(), r#"{ "name" : "x" }"#);
}
