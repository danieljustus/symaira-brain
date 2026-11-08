//! Claude plan label fixtures (#999): formatting, sources, snapshot attach.
use super::{claude_file_plan, claude_from_resolved, claude_plan_from_credentials};
use super::{format_claude_plan, take_claude_keychain_plan};
use crate::transport::{Cancellation, FixtureTransport, Response, Transport};
use std::sync::Arc;

#[test]
fn plan_label_is_title_cased_with_the_rate_limit_multiplier() {
    for (subscription, tier, want) in [
        ("max", Some("default_claude_max_5x"), Some("Max 5x")),
        ("PRO", None, Some("Pro")),
        (" team plan ", Some("tier_20x_extra"), Some("Team Plan 20x")),
        ("max", Some("default_claude_max"), Some("Max")),
        ("max", Some("12y3x"), Some("Max 3x")),
        ("", Some("5x"), None),
        ("  ", None, None),
    ] {
        assert_eq!(format_claude_plan(subscription, tier).as_deref(), want);
    }
}

#[test]
fn plan_is_read_from_claude_code_blob_or_the_selected_account_entry() {
    let blob = br#"{"claudeAiOauth":{"accessToken":"placeholder","subscriptionType":"max","rateLimitTier":"x_5x"}}"#;
    assert_eq!(
        claude_plan_from_credentials(blob).as_deref(),
        Some("Max 5x")
    );
    let accounts = br#"{"oauthAccount":{"work":{"subscriptionType":"team"},"default":{"subscriptionType":"pro"}}}"#;
    assert_eq!(
        claude_plan_from_credentials(accounts).as_deref(),
        Some("Pro")
    );
    let no_default =
        br#"{"oauthAccount":{"b":{"accessToken":"x"},"a":{"subscriptionType":"team"}}}"#;
    assert_eq!(
        claude_plan_from_credentials(no_default).as_deref(),
        Some("Team")
    );
    for bad in [
        &b"{"[..],
        b"[]",
        br#"{"claudeAiOauth":{"subscriptionType":5}}"#,
        b"{}",
    ] {
        assert_eq!(claude_plan_from_credentials(bad), None);
    }
}

#[test]
fn plan_file_read_and_keychain_hand_over_are_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("credentials.json");
    assert_eq!(claude_file_plan(&path), None);
    std::fs::write(
        &path,
        br#"{"claudeAiOauth":{"subscriptionType":"pro","rateLimitTier":"t_20x"}}"#,
    )
    .unwrap();
    assert_eq!(claude_file_plan(&path).as_deref(), Some("Pro 20x"));
    assert_eq!(take_claude_keychain_plan(), None);
}

#[test]
fn oauth_snapshot_carries_the_plan_and_makes_one_request() {
    let body = br#"{"five_hour":{"utilization":10}}"#.to_vec();
    let fixture = FixtureTransport::with_sequences(
        [(
            "claude".to_owned(),
            vec![Ok(Response {
                status: 200,
                body,
                headers: std::collections::BTreeMap::new(),
            })],
        )]
        .into(),
    );
    let transport: Arc<dyn Transport> = Arc::new(fixture.clone());
    let mut provider = claude_from_resolved(
        None,
        None,
        Some(("file".into(), "placeholder".into())),
        None,
        None,
    );
    provider.plan = Some("Max 5x".into());
    let snapshot = provider
        .fetch_claude(&transport, &Cancellation::new())
        .unwrap();
    assert_eq!(snapshot.plan.as_deref(), Some("Max 5x"));
    assert_eq!(fixture.requests().len(), 1);
    let wire = serde_json::to_string(&snapshot).unwrap();
    assert!(wire.contains(r#","plan":"Max 5x""#), "{wire}");
    assert!(!wire.contains("placeholder"));
}
