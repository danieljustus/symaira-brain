//! Actual frozen-Go constructor/report/bytes/state comparisons for device IDs.
use crate::{Request, Response, Service, Transport};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[path = "transport_oracle_tests.rs"]
mod bounds;
#[path = "status_oracle_tests.rs"]
mod status;
#[path = "device_oracle_wire_tests.rs"]
mod wire;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, b| {
        use std::fmt::Write;
        write!(out, "{b:02x}").unwrap();
        out
    })
}
fn request_value(request: &Request) -> Value {
    let headers: BTreeMap<_, _> = request
        .headers
        .iter()
        .map(|(k, v)| (k.to_lowercase(), vec![v.clone()]))
        .collect();
    let raw: BTreeMap<_, _> = request
        .headers
        .iter()
        .map(|(k, v)| (k.to_lowercase(), vec![hex(v.as_bytes())]))
        .collect();
    json!({"method":request.method,"url":request.url,"headers":headers,"header_value_hex":raw,"body_hex":hex(request.body.as_deref().unwrap_or_default())})
}
fn normalize_requests(value: &Value) -> Value {
    let mut value = value.clone();
    for request in value.as_array_mut().unwrap() {
        for field in ["headers", "header_value_hex"] {
            request[field] = Value::Object(
                request[field]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| (k.to_lowercase(), v.clone()))
                    .collect(),
            );
        }
    }
    value
}
#[allow(unsafe_code)]
fn set(name: &str, value: &str) {
    unsafe { std::env::set_var(name, value) };
}
fn prepare(row: &Value) -> BTreeMap<String, Vec<u8>> {
    for name in [
        "ANTHROPIC_ADMIN_KEY",
        "CODEX_ACCESS_TOKEN",
        "COPILOT_ACCESS_TOKEN",
        "CURSOR_COOKIE",
        "KIMI_CODE_API_KEY",
        "KIMI_AUTH_TOKEN",
        "MOONSHOT_API_KEY",
        "NOUS_PORTAL_ACCESS_TOKEN",
        "OPENCODE_COOKIE",
        "OPENROUTER_API_KEY",
        "CODEX_HOME",
        "KIMI_CODE_HOME",
        "HERMES_HOME",
        "KIMI_CODE_BASE_URL",
        "HERMES_PORTAL_BASE_URL",
        "OPENROUTER_API_URL",
        "OPENCODE_WORKSPACE_ID",
        "USAGE_DEVICE_ABSENT",
    ] {
        set(name, "");
    }
    set("ANTHROPIC_OAUTH_TOKEN", "env://USAGE_DEVICE_ABSENT");
    set("HOME", row["home"].as_str().unwrap());
    set("USERPROFILE", row["home"].as_str().unwrap());
    set("PATH", "");
    set("KIMI_CODE_API_KEY", row["input"]["api"].as_str().unwrap());
    set("KIMI_AUTH_TOKEN", row["input"]["web"].as_str().unwrap());
    let root = std::path::Path::new(row["cli_home"].as_str().unwrap());
    let mut files = BTreeMap::new();
    for (name, key) in [
        ("credentials/kimi-code.json", "credential_sha256"),
        ("device_id", "device_sha256"),
    ] {
        if name == "device_id" && !row["input"]["device_present"].as_bool().unwrap() {
            continue;
        }
        let path = root.join(name);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            row[key].as_str().unwrap()
        );
        files.insert(path.to_str().unwrap().to_owned(), bytes);
    }
    files
}
fn readonly(row: &Value, before: &BTreeMap<String, Vec<u8>>) {
    for (path, bytes) in before {
        assert_eq!(
            std::fs::read(path).unwrap(),
            *bytes,
            "{} readonly",
            row["id"]
        );
    }
    if !row["input"]["device_present"].as_bool().unwrap() {
        assert!(
            !std::path::Path::new(row["cli_home"].as_str().unwrap())
                .join("device_id")
                .exists()
        );
    }
}
fn report_value(
    report: crate::Report,
    start: chrono::DateTime<chrono::Utc>,
    end: chrono::DateTime<chrono::Utc>,
) -> Value {
    let mut value = serde_json::to_value(report).unwrap();
    for p in value["providers"].as_array_mut().unwrap() {
        if let Some(snapshot) = p.get_mut("snapshot") {
            let time =
                chrono::DateTime::parse_from_rfc3339(snapshot["fetched_at"].as_str().unwrap())
                    .unwrap()
                    .with_timezone(&chrono::Utc);
            assert!(
                time >= start && time <= end,
                "actual timestamp bounded by invocation"
            );
        }
    }
    normalize_clocks(&mut value);
    value
}
fn expected_report(row: &Value) -> Value {
    let mut report = row["report"].clone();
    normalize_clocks(&mut report);
    report
}
fn normalize_clocks(report: &mut Value) {
    for p in report["providers"].as_array_mut().unwrap() {
        let opencode = p["id"] == "opencode";
        if let Some(snapshot) = p.get_mut("snapshot") {
            if opencode {
                let fetched =
                    chrono::DateTime::parse_from_rfc3339(snapshot["fetched_at"].as_str().unwrap())
                        .unwrap();
                for meter in snapshot["meters"].as_array_mut().unwrap() {
                    if let Some(reset) = meter.get("resets_at") {
                        let reset =
                            chrono::DateTime::parse_from_rfc3339(reset.as_str().unwrap()).unwrap();
                        let delta = (reset - fetched).num_nanoseconds().unwrap();
                        assert!(
                            matches!(delta, 3_600_000_000_000 | 86_400_000_000_000),
                            "actual fixture reset offsets"
                        );
                        meter["resets_at"] = json!({"nanoseconds_after_actual_fetched_at":delta});
                    }
                }
            }
            snapshot["fetched_at"] = json!("actual-invocation-clock");
        }
    }
}

struct Canned {
    row: Value,
    requests: Mutex<Vec<Value>>,
    api: Vec<u8>,
    web: Vec<u8>,
}
impl Transport for Canned {
    fn request(&self, request: Request) -> Result<Response, String> {
        let mut requests = self.requests.lock().unwrap();
        let index = requests.len();
        requests.push(request_value(&request));
        let status =
            u16::try_from(self.row["input"]["responses"][index].as_u64().unwrap()).unwrap();
        let body = if status == 200 {
            if self.row["input"]["kind"] == "cli-invalid-json" {
                b"broken".to_vec()
            } else if request.method == "POST" {
                self.web.clone()
            } else {
                self.api.clone()
            }
        } else {
            b"{}".to_vec()
        };
        let headers = if status == 429 {
            BTreeMap::from([("retry-after".into(), "3".into())])
        } else {
            BTreeMap::new()
        };
        Ok(Response {
            status,
            body,
            headers,
        })
    }
}
#[test]
#[ignore = "requires fresh frozen Go processes and owned TLS peer"]
fn device_oracle_matches_fresh_go() {
    let records: Vec<Value> =
        serde_json::from_slice(&std::fs::read(std::env::var("USAGE_DEVICE_GO").unwrap()).unwrap())
            .unwrap();
    assert_eq!(records.len(), 64);
    let mut observed = Vec::new();
    let mut gated = 0;
    for row in &records {
        let before = prepare(row);
        let gate = row["input"]["kind"] == "retain-go-control";
        assert_eq!(
            super::needs_go_fallback(),
            gate,
            "{} eligibility",
            row["id"]
        );
        if gate {
            gated += 1;
            readonly(row, &before);
            observed.push(json!({"id":row["id"],"gated":true,"read_only":true}));
            continue;
        }
        let transport = Arc::new(Canned {
            row: row.clone(),
            requests: Mutex::new(vec![]),
            api: std::fs::read(std::env::var("USAGE_DEVICE_API").unwrap()).unwrap(),
            web: std::fs::read(std::env::var("USAGE_DEVICE_WEB").unwrap()).unwrap(),
        });
        let start = chrono::Utc::now();
        let report = Service::with_transport(vec![super::kimi()], transport.clone()).report();
        let end = chrono::Utc::now();
        let raw = serde_json::to_value(&report).unwrap();
        let normalized = report_value(report, start, end);
        assert_eq!(
            normalized,
            expected_report(row),
            "{} full actual report except bounded runtime fetched_at",
            row["id"]
        );
        let requests = transport.requests.lock().unwrap().clone();
        assert_eq!(
            json!(requests),
            normalize_requests(&row["requests"]),
            "{} complete request/raw header bytes",
            row["id"]
        );
        readonly(row, &before);
        observed.push(json!({"id":row["id"],"gated":false,"report":raw,"started":start,"finished":end,"requests":requests,"read_only":true}));
    }
    assert_eq!(gated, 7);
    let wired = wire::compare(&records);
    let statuses = status::compare();
    let bounds = bounds::compare();
    std::fs::write(
        std::env::var("USAGE_DEVICE_NATIVE").unwrap(),
        serde_json::to_vec_pretty(
            &json!({"cases":64,"full_reports":57,"gated":7,"records":observed,"wire":wired,"statuses":statuses,"bounds":bounds}),
        )
        .unwrap(),
    )
    .unwrap();
}
