use super::*;
use crate::transport::Response;
use std::sync::Mutex;
struct Capture(Mutex<Vec<Request>>);
impl Transport for Capture {
    fn request(&self, r: Request) -> Result<Response, String> {
        self.0.lock().unwrap().push(r);
        Ok(Response {
            status: 200,
            headers: std::collections::BTreeMap::default(),
            body: br#"{"userStatus":{"planStatus":{"weeklyQuotaRemainingPercent":0}}}"#.to_vec(),
        })
    }
}
#[test]
fn devin_quota_wire_contract() {
    let capture = Arc::new(Capture(Mutex::new(Vec::new())));
    let tr: Arc<dyn Transport> = capture.clone();
    let p = Provider::fixture("devin", "Devin");
    let s = fetch(&p, &tr, &Cancellation::new()).unwrap();
    assert_eq!(s.meters[0].used.as_deref(), Some("100"));
    let requests = capture.0.lock().unwrap();
    let r = &requests[0];
    assert_eq!(r.method, "POST");
    assert_eq!(
        r.url,
        "https://server.codeium.com/exa.seat_management_pb.SeatManagementService/GetUserStatus"
    );
    assert_eq!(r.headers.get("Connect-Protocol-Version").unwrap(), "1");
    assert_eq!(r.headers.get("Content-Type").unwrap(), "application/json");
    assert!(!r.headers.contains_key("Authorization"));
    let body: serde_json::Value = serde_json::from_slice(r.body.as_ref().unwrap()).unwrap();
    assert_eq!(
        body,
        serde_json::json!({"metadata":{"apiKey":"fixture","ideName":"devin","ideVersion":"1.108.2","extensionName":"devin","extensionVersion":"1.108.2","locale":"en"}})
    );
    assert!(!format!("{r:?}").contains("fixture"));
}
#[test]
fn devin_quota_absence_zero_and_exhaustion() {
    for (fields, used) in [
        (r#""weeklyQuotaRemainingPercent":0"#, Some("100")),
        (r#""weeklyQuotaResetAtUnix":"1770648402""#, Some("100")),
        (
            r#""planInfo":{"hideDailyQuota":true},"dailyQuotaRemainingPercent":70"#,
            Some("30"),
        ),
        (
            r#""planInfo":{"hideDailyQuota":true},"dailyQuotaRemainingPercent":70,"weeklyQuotaResetAtUnix":1770648402"#,
            Some("100"),
        ),
        (r#""overageBalanceMicros":"0""#, Some("0")),
        ("", None),
        (
            r#""weeklyQuotaRemainingPercent":"bad","weeklyQuotaResetAtUnix":1770648402"#,
            None,
        ),
        (r#""weeklyQuotaRemainingPercent":null"#, None),
        (r#""weeklyQuotaRemainingPercent":false"#, None),
        (
            r#""weeklyQuotaRemainingPercent":10,"weeklyQuotaResetAtUnix":"oops""#,
            None,
        ),
    ] {
        let body = format!(r#"{{"userStatus":{{"planStatus":{{{fields}}}}}}}"#);
        let result =
            crate::parser::parse_snapshot("devin", "api", body.as_bytes(), chrono::Utc::now());
        if let Some(used) = used {
            assert_eq!(
                result.unwrap().meters[0].used.as_deref(),
                Some(used),
                "{body}"
            );
        } else {
            assert!(result.is_err(), "{body}");
        }
    }
}
#[test]
fn quota_transport_error_is_redacted() {
    struct Failure;
    impl Transport for Failure {
        fn request(&self, _: Request) -> Result<Response, String> {
            Err("fixture-secret".into())
        }
    }
    let tr: Arc<dyn Transport> = Arc::new(Failure);
    for id in ["zai", "devin"] {
        let err = fetch(&Provider::fixture(id, id), &tr, &Cancellation::new()).unwrap_err();
        assert!(!err.to_string().contains("fixture-secret"));
    }
}
