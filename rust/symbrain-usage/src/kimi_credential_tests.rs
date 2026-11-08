//! Offline Kimi credential-path fixtures (#931): API key, CLI token, expiry.
use super::{Provider, kimi_from_resolved};
use crate::transport::{Cancellation, FixtureTransport, Response, Transport};
use std::collections::BTreeMap;
use std::sync::Arc;

const METERS: &str = r#"{"usage":{"used":"3","limit":"100","resetTime":"2026-11-07T00:00:00Z"}}"#;

fn response(status: u16, body: &str) -> Response {
    Response {
        status,
        body: body.as_bytes().to_vec(),
        headers: BTreeMap::new(),
    }
}

fn provider(api: bool, cli: bool, expired: bool) -> Provider {
    let mut provider = kimi_from_resolved(
        api.then(|| ("env".to_owned(), "placeholder-api".to_owned())),
        None,
        cli.then_some("placeholder-cli"),
        None,
        None,
        "https://api.kimi.com".into(),
        None,
    );
    provider.kimi_cli_expires_at = expired.then_some(946_684_800.0);
    provider
}

fn run(provider: &Provider, replies: Vec<Result<Response, String>>) -> (String, usize, bool) {
    let fixture = FixtureTransport::with_sequences([("kimi".to_owned(), replies)].into());
    let transport: Arc<dyn Transport> = Arc::new(fixture.clone());
    let result = provider.fetch_kimi(&transport, &Cancellation::new());
    let (text, ok) = match result {
        Ok(snapshot) => (snapshot.source, true),
        Err(error) => (error.to_string(), false),
    };
    (text, fixture.requests().len(), ok)
}

#[test]
fn valid_api_key_returns_meters_even_when_the_cli_login_is_expired() {
    let (source, requests, ok) = run(&provider(true, true, true), vec![Ok(response(200, METERS))]);
    assert!(ok);
    assert_eq!((source.as_str(), requests), ("api", 1));
}

#[test]
fn valid_cli_token_returns_meters_after_a_rejected_api_key() {
    let (source, requests, ok) = run(
        &provider(true, true, false),
        vec![Ok(response(401, "no")), Ok(response(200, METERS))],
    );
    assert!(ok);
    assert_eq!((source.as_str(), requests), ("cli", 2));
}

#[test]
fn expired_cli_token_alone_is_specific_and_sends_no_request() {
    let (text, requests, ok) = run(&provider(false, true, true), vec![Ok(response(401, "no"))]);
    assert!(!ok);
    assert_eq!(requests, 0);
    assert_eq!(
        text,
        "all AI usage fallbacks failed: Kimi Code CLI login expired — run the Kimi CLI once to refresh it"
    );
}

#[test]
fn rejected_api_key_keeps_the_oracle_text_next_to_the_specific_expiry() {
    let generic = "Kimi rejected the login (HTTP 401). Check the API access or sign in with the Kimi Code CLI again.";
    let (text, requests, ok) = run(&provider(true, true, true), vec![Ok(response(401, "no"))]);
    assert!(!ok);
    assert_eq!(requests, 1);
    assert_eq!(
        text,
        format!(
            "all AI usage fallbacks failed: {generic}; Kimi Code CLI login expired — run the Kimi CLI once to refresh it"
        )
    );
    assert!(!text.contains("placeholder"), "{text}");
}

#[test]
fn server_errors_stay_generic() {
    let (text, _, _) = run(&provider(true, false, false), vec![Ok(response(500, "no"))]);
    assert!(text.contains("Kimi request failed with HTTP 500"), "{text}");
}

#[test]
fn unexpired_cli_token_keeps_the_generic_rejection_text() {
    let (text, requests, _) = run(&provider(false, true, false), vec![Ok(response(401, "no"))]);
    assert_eq!(requests, 1);
    assert!(
        text.contains("Kimi rejected the login (HTTP 401)"),
        "{text}"
    );
}
