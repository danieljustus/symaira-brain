use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::value::RawValue;
use serde_json::{Value, json};
use symbrain_broker::{CallToolResult, ContentBlock, Tool};
use symbrain_gateway::{BackendError, Gateway, GatewayBackend, GatewayError};
use symbrain_mcp::{Request, Server};
use symbrain_policy::profile::parse::parse;

const FIXTURE: &str = include_str!("fixtures/gateway_cases.json");

#[derive(Debug, Deserialize)]
struct Fixture {
    schema_version: u32,
    profile_toml: String,
    child_tools: Vec<FixtureTool>,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct FixtureTool {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    input_schema: Option<Box<RawValue>>,
    #[serde(default)]
    annotations: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct Case {
    id: String,
    #[serde(default)]
    child_behavior: Option<BTreeMap<String, String>>,
    requests: Vec<Value>,
    stdout: String,
    stdout_only_jsonrpc: bool,
    stdout_non_protocol_bytes: usize,
    #[serde(default)]
    error_classifications: Vec<ErrorClassification>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct ErrorClassification {
    request_id: i64,
    category: String,
    retryable: bool,
}

struct FixtureBackend {
    tools: Vec<Tool>,
    behavior: BTreeMap<String, String>,
    calls: Mutex<Vec<(String, Option<String>)>>,
}

impl GatewayBackend for FixtureBackend {
    fn list_tools(&self) -> Result<Vec<Tool>, BackendError> {
        Ok(self.tools.clone())
    }

    fn call_tool(
        &self,
        name: &str,
        arguments: Option<&RawValue>,
    ) -> Result<CallToolResult, BackendError> {
        self.calls.lock().expect("calls lock").push((
            name.to_string(),
            arguments.map(|value| value.get().to_string()),
        ));
        if self
            .behavior
            .get(name)
            .is_some_and(|value| value == "toolerror")
        {
            return Ok(CallToolResult {
                content: vec![ContentBlock {
                    kind: "text".to_string(),
                    text: format!("toolerror: intentional failure for {name}"),
                }],
                is_error: true,
            });
        }
        Ok(CallToolResult {
            content: vec![ContentBlock {
                kind: "text".to_string(),
                text: arguments.map_or_else(|| "{}".to_string(), |value| value.get().to_string()),
            }],
            is_error: false,
        })
    }
}

fn backend(fixture: &Fixture, behavior: Option<&BTreeMap<String, String>>) -> Arc<FixtureBackend> {
    let tools = fixture
        .child_tools
        .iter()
        .map(|tool| Tool {
            name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool.input_schema.as_ref().and_then(|schema| {
                serde_json::from_str::<Value>(schema.get())
                    .ok()
                    .and_then(|value| RawValue::from_string(value.to_string()).ok())
            }),
            annotations: tool.annotations.clone(),
        })
        .collect();
    Arc::new(FixtureBackend {
        tools,
        behavior: behavior.cloned().unwrap_or_default(),
        calls: Mutex::new(Vec::new()),
    })
}

fn gateway(fixture: &Fixture, behavior: Option<&BTreeMap<String, String>>) -> Gateway {
    let profile = parse("gateway-fixture", &fixture.profile_toml).expect("fixture profile");
    let backend = backend(fixture, behavior);
    let backend: Arc<dyn GatewayBackend> = backend;
    Gateway::new(
        profile,
        BTreeMap::from([("vault".to_string(), backend)]),
        "dev",
    )
    .expect("gateway assembly")
}

fn render(gateway: &Gateway, requests: &[Value]) -> String {
    let mut input = Vec::new();
    for request in requests {
        serde_json::to_writer(&mut input, request).expect("request JSON");
        input.push(b'\n');
    }
    let mut output = Vec::new();
    Server::new(gateway)
        .serve_io(input.as_slice(), &mut output)
        .expect("dispatch");
    String::from_utf8(output).expect("UTF-8 response stream")
}

fn normalize_runtime_output(output: String) -> String {
    let marker = r#"\"generated_at\":\""#;
    let Some(start) = output.find(marker) else {
        return output;
    };
    let value_start = start + marker.len();
    let Some(relative_end) = output[value_start..].find(r#"\""#) else {
        return output;
    };
    let value_end = value_start + relative_end;
    format!(
        "{}<runtime>{}",
        &output[..value_start],
        &output[value_end..]
    )
}

fn error_classifications(output: &str) -> Vec<ErrorClassification> {
    output
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|response| {
            let request_id = response.get("id")?.as_i64()?;
            let tool_error = response
                .pointer("/result/isError")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if tool_error {
                Some(ErrorClassification {
                    request_id,
                    category: "tool".to_string(),
                    retryable: false,
                })
            } else if response.get("error").is_some() {
                Some(ErrorClassification {
                    request_id,
                    category: "rpc".to_string(),
                    retryable: false,
                })
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn fixture_is_the_complete_gateway_black_box_contract() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).expect("gateway fixture");
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.cases.len(), 8);

    for case in &fixture.cases {
        let gateway = gateway(&fixture, case.child_behavior.as_ref());
        let stdout = normalize_runtime_output(render(&gateway, &case.requests));
        assert_eq!(stdout, case.stdout, "fixture case {}", case.id);
        assert_eq!(
            error_classifications(&stdout),
            case.error_classifications,
            "fixture classifications {}",
            case.id
        );
        assert!(case.stdout_only_jsonrpc);
        assert_eq!(case.stdout_non_protocol_bytes, 0);
        for line in stdout.lines() {
            let value: Value = serde_json::from_str(line).expect("JSON-RPC line");
            assert_eq!(
                value.get("jsonrpc"),
                Some(&Value::String("2.0".to_string()))
            );
        }
    }
}

#[test]
fn memory_identity_injection_is_caller_wins() {
    let profile = parse(
        "p",
        "[profile]\nname=\"p\"\n[servers.memory]\nenabled=true\nmode=\"read_write\"\n",
    )
    .expect("profile");
    let backend = Arc::new(FixtureBackend {
        tools: vec![Tool {
            name: "memory_search".to_string(),
            description: String::new(),
            input_schema: None,
            annotations: None,
        }],
        behavior: BTreeMap::new(),
        calls: Mutex::new(Vec::new()),
    });
    let calls = Arc::clone(&backend);
    let backend: Arc<dyn GatewayBackend> = backend;
    let gateway = Gateway::new(
        profile,
        BTreeMap::from([("memory".to_string(), backend)]),
        "dev",
    )
    .expect("gateway");
    let request: Request = serde_json::from_value(json!({
        "jsonrpc":"2.0", "id":1, "method":"tools/call",
        "params":{"name":"memory_search","arguments":{"client_id":"caller"}}
    }))
    .expect("request");
    gateway.handle(&request).expect("dispatch");
    let recorded = calls.calls.lock().expect("calls lock");
    assert_eq!(recorded[0].1.as_deref(), Some(r#"{"client_id":"caller"}"#));
}

#[test]
fn identity_injection_covers_missing_unmapped_and_disabled_calls() {
    let profile = parse(
        "p",
        "[profile]\nname=\"p\"\n[servers.memory]\nenabled=true\nmode=\"read_write\"\n",
    )
    .expect("profile");
    let empty = symbrain_gateway::inject_identity(&profile, "memory", None, true)
        .expect("inject")
        .expect("arguments");
    assert_eq!(empty.get(), r#"{"client_id":"p"}"#);

    let raw = RawValue::from_string(r#"{"x":1}"#.to_string()).expect("raw");
    assert_eq!(
        symbrain_gateway::inject_identity(&profile, "vault", Some(&raw), true)
            .expect("unmapped")
            .expect("arguments")
            .get(),
        raw.get()
    );
    assert_eq!(
        symbrain_gateway::inject_identity(&profile, "memory", Some(&raw), false)
            .expect("disabled")
            .expect("arguments")
            .get(),
        raw.get()
    );
}

#[test]
fn routing_joins_all_content_and_classifies_broker_failures() {
    let result = CallToolResult {
        content: vec![
            ContentBlock {
                kind: "text".to_string(),
                text: "one".to_string(),
            },
            ContentBlock {
                kind: "text".to_string(),
                text: "two".to_string(),
            },
        ],
        is_error: false,
    };
    assert_eq!(symbrain_gateway::joined_text(&result), "one\ntwo");
    let timeout = GatewayError::Backend {
        server: "memory".to_string(),
        source: BackendError::Timeout {
            op: "tools/call".to_string(),
        },
    };
    assert_eq!(timeout.classification().category, "timeout");
    assert!(timeout.classification().retryable);
    let closed = GatewayError::Backend {
        server: "memory".to_string(),
        source: BackendError::Closed {
            op: "tools/call".to_string(),
            detail: "EOF".to_string(),
        },
    };
    assert_eq!(closed.classification().category, "closed");
    let rpc = GatewayError::Backend {
        server: "vault".to_string(),
        source: BackendError::Rpc {
            code: -32602,
            message: "invalid params".to_string(),
        },
    };
    assert_eq!(rpc.classification().category, "rpc");
    assert!(!rpc.classification().retryable);
}

#[test]
fn writer_loop_preserves_framed_gateway_responses() {
    let profile = parse(
        "p",
        "[profile]\nname=\"p\"\n[servers.vault]\nenabled=true\nmode=\"full\"\n",
    )
    .expect("profile");
    let gateway = Gateway::new(profile, BTreeMap::new(), "dev").expect("gateway");
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
    let input = format!("Content-Length: {}\r\n\r\n{body}", body.len());
    let mut output = Vec::new();
    Server::new(&gateway)
        .serve_io(input.as_bytes(), &mut output)
        .expect("serve");

    let output = String::from_utf8(output).expect("UTF-8 output");
    assert!(output.starts_with("Content-Length: "));
    let (header, response) = output.split_once("\r\n\r\n").expect("framed response");
    let length = header
        .strip_prefix("Content-Length: ")
        .expect("content length")
        .parse::<usize>()
        .expect("content length integer");
    assert_eq!(length, response.len());
    let value: Value = serde_json::from_str(response).expect("JSON-RPC response");
    assert_eq!(value["jsonrpc"], "2.0");
    assert_eq!(value["id"], 1);
}

#[test]
fn optional_modules_public_handlers_bound_tools_and_deny_unknown_calls() {
    let profile = parse(
        "optional",
        "[profile]\nname=\"optional\"\n\n[servers.operate]\nenabled=true\ntools_allow=[\"version\"]\n\n[servers.scope]\nenabled=true\ntools_allow=[\"scan\"]\n",
    )
    .expect("profile");
    let operate = Arc::new(FixtureBackend {
        tools: vec![
            Tool {
                name: "version".to_string(),
                description: String::new(),
                input_schema: None,
                annotations: None,
            },
            Tool {
                name: "not_allowlisted".to_string(),
                description: String::new(),
                input_schema: None,
                annotations: None,
            },
        ],
        behavior: BTreeMap::new(),
        calls: Mutex::new(Vec::new()),
    });
    let scope = Arc::new(FixtureBackend {
        tools: vec![
            Tool {
                name: "scan".to_string(),
                description: String::new(),
                input_schema: None,
                annotations: None,
            },
            Tool {
                name: "scope_not_allowlisted".to_string(),
                description: String::new(),
                input_schema: None,
                annotations: None,
            },
        ],
        behavior: BTreeMap::new(),
        calls: Mutex::new(Vec::new()),
    });
    let gateway = Gateway::new(
        profile,
        BTreeMap::from([
            ("operate".to_string(), operate as Arc<dyn GatewayBackend>),
            ("scope".to_string(), scope as Arc<dyn GatewayBackend>),
        ]),
        "dev",
    )
    .expect("gateway");
    let names = gateway
        .tools()
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();
    assert!(names.iter().any(|name| name == "version"));
    assert!(names.iter().any(|name| name == "scan"));
    assert!(!names.iter().any(|name| name == "not_allowlisted"));
    assert!(!names.iter().any(|name| name == "scope_not_allowlisted"));

    for (id, name) in [(1, "version"), (2, "scan")] {
        let request: Request = serde_json::from_value(json!({
            "jsonrpc":"2.0", "id":id, "method":"tools/call",
            "params":{"name":name,"arguments":{}}
        }))
        .expect("request");
        let response = gateway.handle(&request).expect("call").expect("response");
        let value = serde_json::to_value(response).expect("response json");
        assert_eq!(value["result"]["isError"], false, "{name}: {value}");
    }

    let request: Request = serde_json::from_value(json!({
        "jsonrpc":"2.0", "id":3, "method":"tools/call",
        "params":{"name":"not_allowlisted","arguments":{}}
    }))
    .expect("request");
    let response = gateway.handle(&request).expect("call").expect("response");
    let value = serde_json::to_value(response).expect("response json");
    assert_eq!(value["error"]["code"], -32601);
}

#[test]
fn optional_modules_enabled_without_allowlist_remain_private() {
    let profile = parse(
        "default",
        "[profile]\nname=\"default\"\n\n[servers.operate]\nenabled=true\n",
    )
    .expect("profile");
    let operate = Arc::new(FixtureBackend {
        tools: vec![Tool {
            name: "version".to_string(),
            description: String::new(),
            input_schema: None,
            annotations: None,
        }],
        behavior: BTreeMap::new(),
        calls: Mutex::new(Vec::new()),
    });
    let gateway = Gateway::new(
        profile,
        BTreeMap::from([("operate".to_string(), operate as Arc<dyn GatewayBackend>)]),
        "dev",
    )
    .expect("gateway");
    assert!(!gateway.tools().iter().any(|tool| tool.name == "version"));
}

#[test]
fn optional_modules_default_disabled_without_public_registration() {
    let profile = parse("default", "[profile]\nname=\"default\"\n").expect("profile");
    let operate = Arc::new(FixtureBackend {
        tools: vec![Tool {
            name: "version".to_string(),
            description: String::new(),
            input_schema: None,
            annotations: None,
        }],
        behavior: BTreeMap::new(),
        calls: Mutex::new(Vec::new()),
    });
    let gateway = Gateway::new(
        profile,
        BTreeMap::from([("operate".to_string(), operate as Arc<dyn GatewayBackend>)]),
        "dev",
    )
    .expect("gateway");
    assert!(!gateway.tools().iter().any(|tool| tool.name == "version"));
}
