use super::*;
use crate::transport::{Response, Transport};
use std::sync::{Arc, Mutex};
struct QuotaTransport {
    requests: Mutex<Vec<Request>>,
    body: &'static str,
}
impl Transport for QuotaTransport {
    fn request(&self, req: Request) -> Result<Response, String> {
        self.requests.lock().unwrap().push(req);
        Ok(Response {
            status: 200,
            headers: std::collections::BTreeMap::default(),
            body: self.body.as_bytes().into(),
        })
    }
}
#[test]
fn zai_quota_request_and_zero() {
    let tr = Arc::new(QuotaTransport {
        requests: Mutex::new(Vec::new()),
        body: r#"{"data":{"limits":[{"type":"CREDIT_LIMIT","unit":3,"number":5,"percentage":0,"nextResetTime":1770648402389},{"type":"TOKENS_LIMIT","unit":6,"number":1,"percentage":40},{"type":"TIME_LIMIT","currentValue":0,"usage":0}]}}"#,
    });
    let transport: Arc<dyn Transport> = tr.clone();
    let s = Provider::fixture("zai", "Z.ai")
        .fetch(&transport, &Cancellation::new())
        .unwrap();
    assert_eq!(s.meters.len(), 3);
    assert_eq!(s.meters[0].used.as_deref(), Some("0"));
    assert_eq!(
        s.meters[0].resets_at.unwrap().timestamp_millis(),
        1_770_648_402_389
    );
    let requests = tr.requests.lock().unwrap();
    let r = &requests[0];
    assert_eq!(r.method, "GET");
    assert_eq!(r.url, "https://api.z.ai/api/monitor/usage/quota/limit");
    assert_eq!(r.headers.get("Authorization").unwrap(), "Bearer fixture");
    assert!(r.body.is_none());
}
#[test]
fn zai_quota_absence_and_malformed() {
    for body in [
        r"{}",
        r#"{"limits":null}"#,
        r#"{"limits":[{"type":"CREDIT_LIMIT","unit":3,"number":5}]}"#,
        r#"{"limits":[{"type":"TIME_LIMIT","currentValue":false,"usage":3}]}"#,
        r#"{"success":false,"limits":[]}"#,
    ] {
        assert!(
            crate::parser::parse_snapshot("zai", "api", body.as_bytes(), Utc::now()).is_err(),
            "{body}"
        );
    }
    assert!(
        crate::parser::parse_snapshot("zai", "api", br#"{"limits":[]}"#, Utc::now())
            .unwrap()
            .meters
            .is_empty()
    );
}
