use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Value, json};
use symbrain_broker::Tool;
use symbrain_gateway::{Gateway, GatewayBackend};
use symbrain_mcp::Request;
use symbrain_policy::profile::parse::parse;
use symbrain_usage::{Provider, Response, Service};

struct EmptyBackend;

impl GatewayBackend for EmptyBackend {
    fn list_tools(&self) -> Result<Vec<Tool>, symbrain_gateway::BackendError> {
        Ok(Vec::new())
    }

    fn call_tool(
        &self,
        _name: &str,
        _arguments: Option<&serde_json::value::RawValue>,
    ) -> Result<symbrain_broker::CallToolResult, symbrain_gateway::BackendError> {
        unreachable!("usage test has no child calls")
    }
}

fn fixture_service() -> Arc<Service> {
    let ids = [
        ("claude", "Claude"),
        ("codex", "Codex"),
        ("copilot", "GitHub Copilot"),
        ("cursor", "Cursor"),
        ("kimi", "Kimi Code"),
        ("moonshot", "Moonshot"),
        ("nous", "Nous Portal"),
        ("opencode", "OpenCode Go"),
        ("openrouter", "OpenRouter"),
        ("antigravity", "Antigravity"),
    ];
    let providers = ids
        .iter()
        .map(|(id, name)| Provider::fixture(id, name))
        .collect();
    let responses = ids
        .iter()
        .map(|(id, _)| {
            (
                (*id).to_string(),
                Response {
                    status: 200,
                    body: b"{}".to_vec(),
                    headers: BTreeMap::new(),
                },
            )
        })
        .collect();
    Arc::new(Service::with_transport(
        providers,
        Arc::new(symbrain_usage::FixtureTransport::new(responses)),
    ))
}

#[test]
fn usage_is_one_policy_filtered_native_tool_and_dispatches_without_identity() {
    let profile = parse(
        "usage",
        "[profile]\nname=\"usage\"\n[servers.usage]\nenabled=true\n",
    )
    .expect("profile");
    let backend: Arc<dyn GatewayBackend> = Arc::new(EmptyBackend);
    let gateway = Gateway::new(
        profile,
        BTreeMap::from([("foreign".to_string(), backend)]),
        "dev",
    )
    .expect("gateway")
    .with_usage_service(fixture_service());

    let listed = gateway.tools();
    let usage = listed
        .iter()
        .find(|tool| tool.name == "get_ai_usage")
        .expect("usage tool");
    assert_eq!(
        usage.description,
        "Fetch AI subscription/token usage across providers (Claude, Codex, Copilot, Cursor, Kimi, Moonshot, Nous Portal, OpenCode, OpenRouter, Antigravity). Returns the schema-versioned usage report. Read-only."
    );
    assert_eq!(
        usage.input_schema.as_deref().expect("schema").get(),
        r#"{"type":"object","properties":{}}"#
    );
    assert_eq!(
        listed
            .iter()
            .filter(|tool| tool.name == "get_ai_usage")
            .count(),
        1
    );

    let request: Request = serde_json::from_value(json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": {"name": "get_ai_usage", "arguments": {"client_id": "caller"}}
    }))
    .expect("request");
    let response = gateway.handle(&request).expect("response").expect("frame");
    let wire: Value =
        serde_json::from_str(response.result.expect("result").get()).expect("result JSON");
    let report: Value =
        serde_json::from_str(wire["content"][0]["text"].as_str().expect("text")).expect("report");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["providers"].as_array().expect("providers").len(), 10);
    assert_eq!(wire["isError"], false);
}

#[test]
fn usage_tools_deny_hides_native_tool_and_returns_method_not_found() {
    let profile = parse(
        "usage",
        "[profile]\nname=\"usage\"\n[servers.usage]\nenabled=true\ntools_deny=[\"get_ai_usage\"]\n",
    )
    .expect("profile");
    let gateway = Gateway::new(profile, BTreeMap::new(), "dev");
    assert!(gateway.is_ok());
    let gateway = gateway.expect("gateway");
    assert!(
        gateway
            .tools()
            .iter()
            .all(|tool| tool.name != "get_ai_usage")
    );
}
