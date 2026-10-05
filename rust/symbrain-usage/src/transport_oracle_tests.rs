//! Actual owned TLS response bounds, including non-success statuses.
use super::*;
pub(super) fn compare() -> Value {
    let mut output = vec![];
    for status in [200, 401] {
        for over in [false, true] {
            let id = format!(
                "transport-bound-{status}-{}",
                if over { "over" } else { "exact" }
            );
            let transport = super::wire::wired(&json!({"id":id}));
            let result = transport.request(Request {
                provider_id: "antigravity".into(),
                method: "GET".into(),
                url: format!(
                    "https://{}/owned-response-bound",
                    std::env::var("USAGE_DEVICE_PEER").unwrap()
                ),
                headers: BTreeMap::new(),
                body: None,
            });
            if over {
                assert_eq!(
                    result.unwrap_err(),
                    "read response: the response body is larger than request limit: 1048577",
                    "actual TLS body over bound"
                );
            } else {
                let response = result.unwrap();
                assert_eq!(response.status, status);
                assert_eq!(
                    response.body,
                    vec![b'x'; crate::transport::MAX_RESPONSE_BYTES]
                );
                assert_eq!(
                    response.headers.get("content-type").unwrap(),
                    "application/json"
                );
            }
            output.push(json!({"id":id,"status":status,"over":over,"passed":true}));
        }
    }
    json!({"cases":4,"records":output,"scope":"Production HTTPS builder/executor exact existing1MiB response boundary for200/401, shared Antigravity transport consumer. No Go report/process-probe equivalence or cancellation claim."})
}
