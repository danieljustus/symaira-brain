use super::*;
use serde_json::Value;
use std::sync::Mutex;

struct Offline {
    case: Value,
    paths: Mutex<Vec<String>>,
}
impl Transport for Offline {
    fn request(&self, request: Request) -> Result<Response, String> {
        assert_eq!(request.method, "GET");
        assert_eq!(
            request.headers.get("Authorization").map(String::as_str),
            Some("Bearer fixture")
        );
        let path = request
            .url
            .strip_prefix("https://openrouter.ai/api/v1")
            .expect("unsafe origin");
        self.paths.lock().unwrap().push(path.into());
        let (field, status) = match path {
            "/credits" => ("credits", "credits_status"),
            "/key" => ("key", "key_status"),
            "/auth/key" => ("legacy", "legacy_status"),
            _ => panic!("unexpected request path"),
        };
        let status = self.case[status].as_i64().unwrap_or(200);
        if status == -1 {
            return Err("fixture-secret-must-not-escape".into());
        }
        Ok(Response {
            status: u16::try_from(status).unwrap(),
            body: serde_json::to_vec(&self.case[field]).unwrap(),
            headers: std::collections::BTreeMap::default(),
        })
    }
}

#[test]
fn openrouter_current_fixtures() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../tests/fixtures/openrouter-current-cases.json"
    ))
    .unwrap();
    for case in cases {
        let transport = Arc::new(Offline {
            case: case.clone(),
            paths: Mutex::default(),
        });
        let provider = Provider::fixture("openrouter", "OpenRouter");
        let service = crate::Service::with_transport(vec![provider], transport.clone());
        let report = service.report();
        let result = &report.providers[0];
        let mut expected_paths = vec!["/credits"];
        if case["error"] != true {
            expected_paths.push(if case["credits_status"] == 404 {
                "/auth/key"
            } else {
                "/key"
            });
        }
        assert_eq!(
            *transport.paths.lock().unwrap(),
            expected_paths,
            "{}",
            case["name"]
        );
        if case["error"] == true {
            assert!(result.snapshot.is_none(), "{}", case["name"]);
            assert!(!result.error.as_deref().unwrap().contains("fixture-secret"));
            continue;
        }
        let snapshot = result
            .snapshot
            .as_ref()
            .unwrap_or_else(|| panic!("{}: {:?}", case["name"], result.error));
        assert_eq!(
            serde_json::to_value(&snapshot.meters).unwrap(),
            case["meters"],
            "{}",
            case["name"]
        );
        assert_eq!(snapshot.balance.as_deref(), case["balance"].as_str());
        assert_eq!(snapshot.source, "api");
        assert_eq!(snapshot.currency.as_deref(), Some("USD"));
    }
}

#[test]
fn openrouter_current_requests_reject_unsafe_overrides() {
    for base in [
        "http://127.0.0.1",
        "https://user:password@evil.example",
        "https://localhost",
        "https://127.0.0.1",
        "https://openrouter.ai\n",
    ] {
        let mut provider = Provider::fixture("openrouter", "OpenRouter");
        provider.base_url = Some(base.into());
        for source in ["credits", "key"] {
            let request = request_for(&provider, source, "fixture", None);
            assert_eq!(
                request.url,
                format!("https://openrouter.ai/api/v1/{source}")
            );
            assert!(!format!("{request:?}").contains("fixture"));
        }
    }
}

#[test]
fn openrouter_current_preserves_credits_near_deadline() {
    let transport = Arc::new(Offline {
        case: serde_json::json!({"credits":{"data":{"total_credits":10,"total_usage":1}}}),
        paths: Mutex::default(),
    });
    let report = crate::Service::with_transport(
        vec![Provider::fixture("openrouter", "OpenRouter")],
        transport.clone(),
    )
    .with_provider_timeout(std::time::Duration::from_millis(50))
    .report();
    assert_eq!(
        report.providers[0]
            .snapshot
            .as_ref()
            .unwrap()
            .balance
            .as_deref(),
        Some("9")
    );
    assert_eq!(*transport.paths.lock().unwrap(), vec!["/credits"]);
}
