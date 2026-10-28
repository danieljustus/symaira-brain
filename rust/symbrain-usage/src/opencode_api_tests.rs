use super::*;
use crate::{FixtureTransport, Response};
use std::collections::BTreeMap;

const BODY: &str = r#"{"usage":{"rolling":{"percent":0,"resetsAt":"2026-10-28T05:00:00Z"},"weekly":{"percent":23.5,"resetsAt":"2026-11-02T00:00:00Z"},"monthly":{"percent":100,"resetsAt":"2026-11-20T00:00:00Z"}}}"#;
#[test]
fn opencode_api_parser_preserves_zero_absence_and_server_resets() {
    let now = "2026-10-28T00:00:00Z".parse().unwrap();
    let s = parse(BODY.as_bytes(), now, now).unwrap();
    assert_eq!(s.meters.len(), 3);
    assert_eq!(s.meters[0].used.as_deref(), Some("0"));
    assert!(s.meters[0].resets_at.is_none());
    assert_eq!(s.meters[1].used.as_deref(), Some("23.5"));
    assert_eq!(s.meters[2].label, "Monthly");
    assert!(s.meters[1].resets_at.is_some());
    let partial = parse(
        br#"{"usage":{"rolling":{"resetsAt":"2026-10-28T01:00:00Z"},"weekly":{"percent":0}}}"#,
        now,
        now,
    )
    .unwrap();
    assert!(partial.meters[0].used.is_none());
    assert_eq!(partial.meters[1].used.as_deref(), Some("0"));
    let started = parse(BODY.replace("05:00:00", "04:59:57").as_bytes(), now, now).unwrap();
    assert!(started.meters[0].resets_at.is_some());
    for body in [
        "{}",
        r#"{"usage":{}}"#,
        r#"{"usage":{"weekly":{"percent":true}}}"#,
        r#"{"usage":{"weekly":{"percent":"oops"}}}"#,
        r#"{"usage":{"weekly":{"percent":1,"resetsAt":"oops"}}}"#,
    ] {
        assert!(parse(body.as_bytes(), now, now).is_err());
    }
}
#[test]
fn opencode_api_transport_never_downgrades() {
    for status in [200, 301, 401, 403, 404, 500] {
        let fixture = FixtureTransport::new(BTreeMap::from([(
            "opencode".into(),
            Response {
                status,
                body: BODY.as_bytes().to_vec(),
                headers: BTreeMap::new(),
            },
        )]));
        let transport: Arc<dyn Transport> = Arc::new(fixture.clone());
        let result = fetch("synthetic-key", &transport, &Cancellation::new());
        assert_eq!(result.is_ok(), status == 200);
        let requests = fixture.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url, URL);
        assert_eq!(requests[0].method, "GET");
        assert_eq!(
            requests[0].headers.get("Authorization").unwrap(),
            "Bearer synthetic-key"
        );
        assert!(!requests[0].headers.contains_key("Cookie"));
    }
}
#[test]
fn opencode_api_transport_redacts_errors_and_bounds_bodies() {
    let fixture = FixtureTransport::with_errors(BTreeMap::from([(
        "opencode".into(),
        Err("synthetic-secret transport leak".into()),
    )]));
    let transport: Arc<dyn Transport> = Arc::new(fixture);
    assert!(
        !fetch("synthetic-secret", &transport, &Cancellation::new())
            .unwrap_err()
            .to_string()
            .contains("synthetic-secret")
    );
    let fixture = FixtureTransport::new(BTreeMap::from([(
        "opencode".into(),
        Response {
            status: 200,
            body: vec![b'x'; crate::transport::MAX_RESPONSE_BYTES + 1],
            headers: BTreeMap::new(),
        },
    )]));
    let transport: Arc<dyn Transport> = Arc::new(fixture);
    assert!(fetch("synthetic-key", &transport, &Cancellation::new()).is_err());
}
