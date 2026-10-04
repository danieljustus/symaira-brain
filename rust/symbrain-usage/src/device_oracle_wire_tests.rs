//! Private resolver/trust roots; unchanged production HTTPS request execution.
use super::*;
use std::net::SocketAddr;
use std::time::Duration;
use ureq::unversioned::resolver::{ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

#[derive(Debug)]
struct OwnedResolver(SocketAddr);
impl Resolver for OwnedResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        _config: &ureq::config::Config,
        _timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        if !matches!(
            uri.host(),
            Some(
                "127.0.0.1"
                    | "api.kimi.com"
                    | "www.kimi.com"
                    | "api.anthropic.com"
                    | "chatgpt.com"
                    | "api.github.com"
                    | "cursor.com"
                    | "api.moonshot.ai"
                    | "portal.nousresearch.com"
                    | "opencode.ai"
                    | "openrouter.ai"
            )
        ) {
            return Err(ureq::Error::HostNotFound);
        }
        let mut addresses = self.empty();
        addresses.push(self.0);
        Ok(addresses)
    }
}
pub(super) struct Wired {
    inner: crate::UreqTransport,
    row: Value,
    pub(super) requests: Mutex<Vec<Value>>,
}
impl Transport for Wired {
    fn request(&self, mut request: Request) -> Result<Response, String> {
        let mut requests = self.requests.lock().unwrap();
        let index = requests.len();
        requests.push(request_value(&request));
        request.headers.insert(
            "X-Owned-Case".into(),
            self.row["id"].as_str().unwrap().into(),
        );
        request
            .headers
            .insert("X-Owned-Index".into(), index.to_string());
        self.inner.request(request)
    }
}
pub(super) fn compare(records: &[Value]) -> Value {
    let fixture = std::env::var("USAGE_DEVICE_WIRE_GO").expect("actual TLS Go evidence");
    let expected: Vec<Value> = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    assert_eq!(expected.len(), 57);
    let mut output = vec![];
    for row in records
        .iter()
        .filter(|row| row["input"]["kind"] != "retain-go-control")
    {
        let before = prepare(row);
        assert!(!super::super::needs_go_fallback());
        let transport = Arc::new(wired(row));
        let start = chrono::Utc::now();
        let report =
            Service::with_transport(vec![super::super::kimi()], transport.clone()).report();
        let end = chrono::Utc::now();
        let raw = serde_json::to_value(&report).unwrap();
        let normalized = report_value(report, start, end);
        let go = expected.iter().find(|go| go["id"] == row["id"]).unwrap();
        assert_eq!(
            normalized,
            expected_report(go),
            "{} actual TLS full report except bounded invocation clock",
            row["id"]
        );
        let requests = transport.requests.lock().unwrap().clone();
        assert_eq!(
            json!(requests),
            normalize_requests(&go["requests"]),
            "{} actual TLS request/rawheaders",
            row["id"]
        );
        readonly(row, &before);
        output.push(json!({"id":row["id"],"report":raw,"started":start,"finished":end,"requests":requests,"read_only":true}));
    }
    json!({"cases":57,"records":output,"tls_verified":true,"private_dns_only":true,"deadline_claim":false})
}

pub(super) fn wired(row: &Value) -> Wired {
    let ca = std::fs::read(std::env::var("USAGE_DEVICE_CA").unwrap()).unwrap();
    let cert = ureq::tls::Certificate::from_pem(&ca).unwrap();
    let config = crate::UreqTransport::agent_config(Duration::from_secs(8))
        .proxy(None)
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .root_certs(ureq::tls::RootCerts::new_with_certs(std::slice::from_ref(
                    &cert,
                )))
                .build(),
        )
        .build();
    let agent = ureq::Agent::with_parts(
        config,
        DefaultConnector::default(),
        OwnedResolver(std::env::var("USAGE_DEVICE_PEER").unwrap().parse().unwrap()),
    );
    // The wrapper retains private trust/DNS on cooperative request dispatch.
    // This fast-peer proof does not claim cancellation/deadline parity.
    Wired {
        inner: crate::UreqTransport::with_owned_test_agent(agent),
        row: row.clone(),
        requests: Mutex::new(vec![]),
    }
}
