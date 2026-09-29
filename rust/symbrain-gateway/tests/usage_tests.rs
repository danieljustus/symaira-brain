use std::collections::BTreeMap;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Command;
use std::sync::Arc;
#[cfg(unix)]
use std::sync::atomic::AtomicBool;

use serde_json::{Value, json};
use symbrain_broker::Tool;
use symbrain_gateway::{Gateway, GatewayBackend};
#[cfg(unix)]
use symbrain_mcp::DispatchContext;
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

#[cfg(unix)]
#[test]
fn production_usage_discovery_is_lazy_and_per_call() {
    let directory = TestTempDir::new();
    let home = directory.path().join("home");
    let bin = directory.path().join("bin");
    fs::create_dir_all(&home).expect("create isolated home");
    fs::create_dir_all(&bin).expect("create fake command directory");
    let marker = directory.path().join("ps-invocations");
    let ps = bin.join("ps");
    fs::write(
        &ps,
        b"#!/bin/sh\nprintf x >> \"$SYMBRAIN_TEST_PS_MARKER\"\n",
    )
    .expect("write synthetic process probe");
    fs::set_permissions(&ps, fs::Permissions::from_mode(0o700))
        .expect("make synthetic process probe executable");

    let output = Command::new(std::env::current_exe().expect("test executable"))
        .arg("production_usage_discovery_helper")
        .arg("--nocapture")
        .env_clear()
        .env("SYMBRAIN_USAGE_DISCOVERY_HELPER", "1")
        .env("SYMBRAIN_TEST_PS_MARKER", &marker)
        .env("HOME", &home)
        .env("CODEX_HOME", home.join(".codex"))
        .env("ANTHROPIC_OAUTH_TOKEN", "synthetic-no-keychain-token")
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .output()
        .expect("run isolated production gateway helper");
    assert!(
        output.status.success(),
        "isolated helper failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn production_usage_discovery_helper() {
    if std::env::var_os("SYMBRAIN_USAGE_DISCOVERY_HELPER").is_none() {
        return;
    }
    let marker =
        PathBuf::from(std::env::var_os("SYMBRAIN_TEST_PS_MARKER").expect("private probe marker"));
    let home = PathBuf::from(std::env::var_os("HOME").expect("private HOME"));
    let profile = parse(
        "usage-lazy",
        "[profile]\nname=\"usage-lazy\"\n[servers.usage]\nenabled=true\n",
    )
    .expect("profile");
    let gateway = Gateway::new(profile, BTreeMap::new(), "dev").expect("gateway");
    assert!(
        gateway
            .tools()
            .iter()
            .any(|tool| tool.name == "get_ai_usage")
    );
    assert!(
        !marker.exists(),
        "Gateway::new must not discover providers or probe processes"
    );

    let request: Request = serde_json::from_value(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {"name": "get_ai_usage", "arguments": {}}
    }))
    .expect("request");
    let cancelled = AtomicBool::new(true);

    let first = gateway
        .handle_with_context(&request, DispatchContext::new(&cancelled))
        .expect("first call")
        .expect("first response");
    assert_eq!(fs::read(&marker).expect("first process probe").len(), 1);
    assert_eq!(codex_configured(&first), Some(false));

    let codex_dir = home.join(".codex");
    fs::create_dir_all(&codex_dir).expect("create synthetic Codex directory");
    fs::write(
        codex_dir.join("auth.json"),
        br#"{"tokens":{"access_token":"synthetic-codex-file-token"}}"#,
    )
    .expect("write synthetic Codex auth file");

    let second = gateway
        .handle_with_context(&request, DispatchContext::new(&cancelled))
        .expect("second call")
        .expect("second response");
    assert_eq!(fs::read(&marker).expect("second process probe").len(), 2);
    assert_eq!(codex_configured(&second), Some(true));
}

#[cfg(unix)]
fn codex_configured(response: &symbrain_gateway::GatewayResponse) -> Option<bool> {
    let wire: Value = serde_json::from_str(response.result.as_ref()?.get()).ok()?;
    let report: Value = serde_json::from_str(wire["content"][0]["text"].as_str()?).ok()?;
    report["providers"]
        .as_array()?
        .iter()
        .find(|provider| provider["id"] == "codex")?["configured"]
        .as_bool()
}

#[cfg(unix)]
struct TestTempDir(PathBuf);

#[cfg(unix)]
impl TestTempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "symbrain-gateway-usage-lazy-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("create private fixture directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

#[cfg(unix)]
impl Drop for TestTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
