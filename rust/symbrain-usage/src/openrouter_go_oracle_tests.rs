//! Current acceptance consumes actual Go process observations, separate from frozen legacy evidence.
use super::provider_config::openrouter_from_resolved;
use crate::{FixtureTransport, Response, Service};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

const CAPTURE: &[u8] = include_bytes!("../tests/fixtures/openrouter_go_v1.json");
const SHA256: &str = "cfe8edcb7f1e3facc8fed10801e92e0ad6fee8177d914e5268bd47931b24dbb1";

fn compare(bytes: &[u8]) -> Result<(), String> {
    if format!("{:x}", Sha256::digest(bytes)) != SHA256 {
        return Err("Go capture integrity mismatch".into());
    }
    let capture: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    assert_eq!(capture["schema_version"], 1);
    let cases = capture["cases"].as_array().expect("Go cases");
    assert_eq!(cases.len(), 27);
    let mut ids = BTreeSet::new();
    let mut observations = Vec::new();
    for case in cases {
        let input = &case["input"];
        let id = input["id"].as_str().expect("case ID");
        assert!(ids.insert(id));
        let replies = input["replies"].as_array().expect("scripted replies");
        let sequence = replies
            .iter()
            .map(|reply| {
                if reply["status"] == -1 {
                    Err("oracle-only-invalid-transport-detail".into())
                } else {
                    Ok(Response {
                        status: u16::try_from(reply["status"].as_u64().expect("HTTP status"))
                            .unwrap(),
                        body: reply["body"]
                            .as_str()
                            .expect("raw response body")
                            .as_bytes()
                            .to_vec(),
                        headers: serde_json::from_value(reply["headers"].clone())
                            .expect("reply headers"),
                    })
                }
            })
            .collect();
        let transport = FixtureTransport::with_sequences([("openrouter".into(), sequence)].into());
        let base = input["base"].as_str().expect("base override");
        let provider = openrouter_from_resolved(
            Some(("env".into(), "oracle-only-invalid-v1-credential".into())),
            None,
            if base.is_empty() {
                "https://openrouter.ai/api/v1"
            } else {
                base
            }
            .into(),
        );
        let mut report =
            Service::with_transport(vec![provider], Arc::new(transport.clone())).report();
        if let Some(snapshot) = &mut report.providers[0].snapshot {
            snapshot.fetched_at = chrono::DateTime::UNIX_EPOCH;
        }
        let requests: Vec<Value> = transport
            .requests()
            .iter()
            .map(|r| {
                let headers: BTreeMap<String, Vec<String>> = r
                    .headers
                    .iter()
                    .map(|(k, v)| (k.to_ascii_lowercase(), vec![v.clone()]))
                    .collect();
                json!({"method":r.method,"url":r.url,"headers":headers,
                "body":String::from_utf8(r.body.clone().unwrap_or_default()).unwrap()})
            })
            .collect();
        let actual = json!({"input":input,"requests":requests,"report":report});
        if actual != *case {
            return Err(format!(
                "{id}: complete Go/Rust observation mismatch\nRust: {actual}\nGo: {case}"
            ));
        }
        observations.push(actual);
    }
    if let Some(path) = std::env::var_os("OPENROUTER_NATIVE_V1") {
        std::fs::write(path, serde_json::to_vec_pretty(&observations).unwrap()).unwrap();
    }
    Ok(())
}

#[test]
fn current_openrouter_v1_complete_reports_and_requests_match_executed_go() {
    let bytes = std::env::var_os("OPENROUTER_GO_V1").map_or_else(
        || CAPTURE.to_vec(),
        |path| std::fs::read(path).expect("fresh immutable Go capture"),
    );
    compare(&bytes).unwrap();
}

#[test]
fn current_openrouter_v1_rejects_mutated_or_missing_observations() {
    compare(CAPTURE).unwrap();
    let original: Value = serde_json::from_slice(CAPTURE).unwrap();
    for field in ["report", "requests", "input"] {
        let mut mutated = original.clone();
        mutated["cases"][0].as_object_mut().unwrap().remove(field);
        assert_eq!(
            compare(&serde_json::to_vec(&mutated).unwrap()).unwrap_err(),
            "Go capture integrity mismatch"
        );
    }
    let mut mutated = original;
    mutated["cases"][0]["report"]["providers"][0]["snapshot"]["balance"] = "wrong".into();
    assert!(compare(&serde_json::to_vec(&mutated).unwrap()).is_err());
    assert!(compare(b"{}").is_err());
}
