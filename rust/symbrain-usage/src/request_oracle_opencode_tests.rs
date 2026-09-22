//! Full legacy `OpenCode` request walk; shared corpus helpers remain in parent.
use super::{Case, headers_of, normalize};
use crate::providers::Provider;
use crate::transport::{Cancellation, FixtureTransport, Response};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

/// Replays the shipped `OpenCode` strategy's full walk: the recorder answers
/// every request with `200 {}`, so the shipped code GETs the workspaces
/// server, POSTs the `[]` fallback, and fails with `missing workspace id`.
/// The port must execute exactly that sequence. The Go recorder captures the
/// strategy error directly; `Provider::fetch` adds the chain wrapper that the
/// chain runner would add in production, hence the explicit prefix.
pub(super) fn replay_opencode_walk(case: &Case, entry: &Value, provider: &Provider) {
    let mut provider = provider.clone();
    provider.fixture = false;
    let answer = Ok(Response {
        status: 200,
        body: b"{}".to_vec(),
        headers: BTreeMap::new(),
    });
    let fixture = FixtureTransport::with_sequences(
        [("opencode".to_string(), vec![answer.clone(), answer])]
            .into_iter()
            .collect(),
    );
    let transport: Arc<dyn crate::transport::Transport> = Arc::new(fixture.clone());
    let cancel = Cancellation::with_timeout(Duration::from_secs(5));
    let result = provider.fetch(&transport, &cancel);

    let recorded = entry["requests"]
        .as_array()
        .unwrap_or_else(|| panic!("{}/{}: requests", case.provider, case.source));
    let executed = fixture.requests();
    assert_eq!(
        executed.len(),
        recorded.len(),
        "{}: executed request count",
        case.provider
    );
    for (index, request) in executed.iter().enumerate() {
        let shipped = &recorded[index];
        assert_eq!(
            request.method, shipped["method"],
            "{}[{index}]: method",
            case.provider
        );
        assert_eq!(
            request.url,
            normalize(shipped["url"].as_str().expect("url")),
            "{}[{index}]: url",
            case.provider
        );
        let body = request.body.as_ref().map_or_else(String::new, |bytes| {
            String::from_utf8_lossy(bytes).into_owned()
        });
        assert_eq!(
            body,
            shipped["body"].as_str().unwrap_or_default(),
            "{}[{index}]: body",
            case.provider
        );

        let mut expected = headers_of(shipped);
        let shipped_instance = expected.remove("x-server-instance");
        let mut ported: BTreeMap<String, String> = request
            .headers
            .iter()
            .map(|(name, value)| (name.to_ascii_lowercase(), normalize(value)))
            .collect();
        let ported_instance = ported.remove("x-server-instance");
        assert_eq!(
            ported_instance.map(|value| value.starts_with("server-fn:")),
            shipped_instance.map(|value| value.starts_with("server-fn:")),
            "{}[{index}]: X-Server-Instance shape",
            case.provider
        );
        assert_eq!(ported, expected, "{}[{index}]: headers", case.provider);
    }

    let error = result
        .expect_err("the recorder's `200 {}` answer carries no workspace id")
        .to_string();
    let strategy_error = entry["error"]
        .as_str()
        .unwrap_or_else(|| panic!("{}/{}: error", case.provider, case.source));
    assert_eq!(
        error,
        format!("all AI usage fallbacks failed: {strategy_error}"),
        "{}: recorded strategy error",
        case.provider
    );
}
