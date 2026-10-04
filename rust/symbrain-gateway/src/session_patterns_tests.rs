//! Recording boundaries follow the pinned Go handler wrappers, not exposure policy.
use super::*;
use crate::{BackendError, GatewayBackend};
use serde_json::value::RawValue;
use std::collections::BTreeMap;
use std::sync::Arc;
use symbrain_broker::{CallToolResult, Tool};

struct Child;
impl GatewayBackend for Child {
    fn list_tools(&self) -> Result<Vec<Tool>, BackendError> {
        Ok(["health", "get_entry", "hidden"]
            .into_iter()
            .map(|name| Tool {
                name: name.into(),
                description: String::new(),
                input_schema: None,
                annotations: None,
            })
            .collect())
    }
    fn call_tool(
        &self,
        name: &str,
        _arguments: Option<&RawValue>,
    ) -> Result<CallToolResult, BackendError> {
        if name == "get_entry" {
            return Err(BackendError::Timeout { op: "call".into() });
        }
        Ok(CallToolResult {
            content: Vec::new(),
            is_error: false,
        })
    }
}
fn gateway(enabled: bool) -> Gateway {
    let profile = symbrain_policy::profile::parse::parse(
        "owned",
        r#"
[profile]
name="owned"
[servers.vault]
enabled=true
mode="full"
tools_deny=["hidden"]
[servers.memory]
enabled=false
[servers.skills]
enabled=false
[servers.usage]
enabled=false
[audit]
enabled=false
"#,
    )
    .unwrap();
    let child: Arc<dyn GatewayBackend> = Arc::new(Child);
    Gateway::new(profile, BTreeMap::from([("vault".into(), child)]), "owned")
        .unwrap()
        .with_patterns(enabled, 5)
}
fn call(gateway: &Gateway, name: &str) {
    let request = serde_json::from_value(serde_json::json!({
        "jsonrpc":"2.0", "id":1, "method":"tools/call",
        "params":{"name":name,"arguments":{"secret":"must-not-be-stored"}}
    }))
    .unwrap();
    gateway.handle(&request).unwrap();
}

#[test]
fn only_completed_exposed_calls_record_names_including_child_errors() {
    let gateway = gateway(true);
    for name in [
        "bootstrap",
        "vault_health",
        "vault_get_entry",
        "vault_hidden",
        "unknown",
    ] {
        call(&gateway, name);
    }
    let recorder = gateway.episode.lock().unwrap();
    let episode = recorder.as_ref().unwrap();
    assert_eq!(
        episode.steps,
        [
            Step {
                server: "vault".into(),
                tool: "health".into()
            },
            Step {
                server: "vault".into(),
                tool: "get_entry".into()
            },
        ]
    );
    assert!(
        !serde_json::to_string(episode)
            .unwrap()
            .contains("must-not-be-stored")
    );
    assert_eq!(gateway.pattern_threshold, 5);
}

#[test]
fn disabling_recording_preserves_calls_and_default_threshold_resolution() {
    let gateway = gateway(false).with_patterns(false, 0);
    call(&gateway, "vault_health");
    assert!(gateway.episode.lock().unwrap().is_none());
    assert_eq!(gateway.pattern_threshold, 3);
}
