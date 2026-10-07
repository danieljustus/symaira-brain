use super::Provider;
use crate::AuthStatus;
use crate::transport::{FixtureTransport, Request, Response, Transport};

const CREDENTIAL: &str = "debug-only-secret-sentinel";
const URL: &str = "https://provider.invalid/debug-only-url-sentinel";
const DEVICE_ID: &str = "debug-only-device-sentinel";
const ENTERPRISE_HOST: &str = "debug-only-enterprise-host-sentinel.invalid";
const SENSITIVE_VALUES: [&str; 4] = [CREDENTIAL, URL, DEVICE_ID, ENTERPRISE_HOST];

#[test]
fn debug_formats_redact_secrets_without_mutating_values() {
    let mut provider = Provider::new(
        "test-provider",
        "Debug fixture",
        Vec::new(),
        vec![("api".into(), CREDENTIAL.into())],
        AuthStatus {
            status: "available".into(),
            detail: "test fixture".into(),
            source: Some("fixture".into()),
        },
    );
    provider.base_url = Some(URL.into());
    provider.device_id = Some(DEVICE_ID.into());
    provider.enterprise_host = Some(ENTERPRISE_HOST.into());

    let request = Request {
        provider_id: "test-provider".into(),
        method: "POST".into(),
        url: URL.into(),
        headers: [("authorization".into(), CREDENTIAL.into())].into(),
        body: Some(CREDENTIAL.as_bytes().to_vec()),
    };
    let response = Response {
        status: 200,
        body: CREDENTIAL.as_bytes().to_vec(),
        headers: [("x-debug".into(), CREDENTIAL.into())].into(),
    };
    let fixture = FixtureTransport::new([("test-provider".into(), response.clone())].into());
    let nested = (&provider, &request, &response, &fixture);

    let nested_output = format!("{nested:?}\n{nested:#?}");
    for sentinel in SENSITIVE_VALUES {
        assert!(
            !nested_output.contains(sentinel),
            "Debug output leaked {sentinel}: {nested_output}"
        );
    }
    // A byte body can leak as decimal numbers without containing the text sentinel.
    for output in [
        format!("{request:?}"),
        format!("{request:#?}"),
        format!("{response:?}"),
        format!("{response:#?}"),
    ] {
        for field in ["headers", "body"] {
            assert!(output.contains(&format!("{field}: \"[REDACTED]\"")));
        }
    }

    assert_eq!(
        (
            provider.credential.as_deref(),
            provider.credentials[0].1.as_str(),
            provider.base_url.as_deref(),
            provider.device_id.as_deref(),
            provider.enterprise_host.as_deref(),
        ),
        (
            Some(CREDENTIAL),
            CREDENTIAL,
            Some(URL),
            Some(DEVICE_ID),
            Some(ENTERPRISE_HOST),
        )
    );
    assert_eq!(
        (
            request.url.as_str(),
            request.headers.get("authorization").map(String::as_str),
            request.body.as_deref(),
        ),
        (URL, Some(CREDENTIAL), Some(CREDENTIAL.as_bytes()))
    );
    assert_eq!(
        (
            response.body.as_slice(),
            response.headers.get("x-debug").map(String::as_str),
        ),
        (CREDENTIAL.as_bytes(), Some(CREDENTIAL))
    );

    let fixture_response = fixture.request(request.clone()).expect("fixture response");
    assert_eq!(
        (
            fixture_response.body.as_slice(),
            fixture_response.headers.get("x-debug").map(String::as_str),
        ),
        (CREDENTIAL.as_bytes(), Some(CREDENTIAL))
    );
    let recorded_request = fixture.requests();
    assert_eq!(recorded_request.len(), 1);
    assert_eq!(
        (
            recorded_request[0].url.as_str(),
            recorded_request[0]
                .headers
                .get("authorization")
                .map(String::as_str),
            recorded_request[0].body.as_deref(),
        ),
        (URL, Some(CREDENTIAL), Some(CREDENTIAL.as_bytes()))
    );
}
