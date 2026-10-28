use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
use symbrain_gateway::Gateway;
use symbrain_mcp::Request;
use symbrain_policy::profile::parse::parse;
use symbrain_usage::{FixtureTransport, Provider, Response, Service};

#[test]
fn grok_weekly_quota_is_returned_by_native_mcp() {
    let transport = FixtureTransport::new(BTreeMap::from([("grok".into(), Response {
        status: 200,
        body: br#"{"config":{"creditUsagePercent":0,"currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY","start":"2026-07-03T00:00:00Z","end":"2026-07-10T00:00:00Z"},"onDemandCap":{"val":2500}}}"#.into(),
        headers: BTreeMap::new(),
    })]));
    let profile = parse(
        "grok",
        "[profile]\nname=\"grok\"\n[servers.usage]\nenabled=true\n",
    )
    .unwrap();
    let gateway = Gateway::new(profile, BTreeMap::new(), "test")
        .unwrap()
        .with_usage_service(Arc::new(Service::with_transport(
            vec![Provider::fixture("grok", "Grok")],
            Arc::new(transport.clone()),
        )));
    let request: Request = serde_json::from_value(json!({
        "jsonrpc":"2.0", "id":1, "method":"tools/call", "params":{"name":"get_ai_usage","arguments":{}}
    })).unwrap();
    let response = gateway.handle(&request).unwrap().unwrap();
    let wire: Value = serde_json::from_str(response.result.unwrap().get()).unwrap();
    let report: Value = serde_json::from_str(wire["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(wire["isError"], false);
    assert_eq!(report["schema_version"], 1);
    let provider = &report["providers"][0];
    assert_eq!(provider["id"], "grok");
    assert!(provider.get("error").is_none());
    assert_eq!(provider["snapshot"]["meters"].as_array().unwrap().len(), 1);
    assert_eq!(provider["snapshot"]["meters"][0]["used"], "0");
    assert_eq!(provider["snapshot"]["meters"][0]["unit"], "%");
    assert_eq!(transport.requests().len(), 1);
}
