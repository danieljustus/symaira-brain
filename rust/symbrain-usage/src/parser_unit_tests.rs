//! Provider-neutral parser edges: body errors, windows, times and quotas.
use super::parse_snapshot;
use crate::{UsageError, UsageMeter};
use chrono::{DateTime, Utc};

fn now() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).unwrap()
}

fn meters(id: &str, body: &str) -> Vec<UsageMeter> {
    parse_snapshot(id, "oauth", body.as_bytes(), now())
        .unwrap_or_else(|error| panic!("{id}: {error:?}"))
        .meters
}

fn reset(meter: &UsageMeter) -> Option<String> {
    meter.resets_at.map(|time| time.to_rfc3339())
}

#[test]
fn empty_unparseable_and_unknown_bodies_use_provider_wording() {
    let parse = |id: &str, detail: &str| UsageError::Parse {
        provider: id.into(),
        detail: detail.into(),
    };
    assert_eq!(
        parse_snapshot("claude", "oauth", b"", now()).unwrap_err(),
        parse("claude", "empty response")
    );
    assert_eq!(
        parse_snapshot("cursor", "web", b"", now()).unwrap_err(),
        parse("cursor", "usage summary is not JSON")
    );
    assert_eq!(
        parse_snapshot("codex", "oauth", b"{", now()).unwrap_err(),
        parse("codex", "response is not JSON")
    );
    assert_eq!(
        parse_snapshot("cursor", "web", b"<html>", now()).unwrap_err(),
        parse("cursor", "usage summary is not JSON")
    );
    assert_eq!(
        parse_snapshot("unknown", "oauth", b"{}", now()).unwrap_err(),
        parse("unknown", "unknown provider")
    );
    assert_eq!(
        parse_snapshot("codex", "oauth", b"{}", now()).unwrap_err(),
        UsageError::Payload {
            provider: "codex".into(),
            detail: "response contained no usable usage fields".into(),
        }
    );
    // Claude's Admin API strategy always reports USD, even without totals.
    let api = parse_snapshot("claude", "api", b"{}", now()).unwrap();
    assert!(api.meters.is_empty());
    assert_eq!(api.currency.as_deref(), Some("USD"));
}

#[test]
fn codex_window_labels_resets_and_additional_limits() {
    let body = r#"{
        "rate_limit": {
            "primary_window": {"used_percent": 12.5, "limit_window_seconds": 18000, "reset_at": 1700003600},
            "secondary_window": {"utilized": "40", "limit_window_seconds": 604800, "reset_date": "2023-11-20"}
        },
        "additional_rate_limits": [
            {"limit_name": "GPT-5", "rate_limit": {"primary_window": {"used_percent": -0.0, "limit_window_seconds": 172800}}},
            {"title": "Credits", "utilized": 3, "limit": 10, "reset_date": 1700000000000},
            {"window": "Monthly", "utilized": 1, "limit": 2},
            {"utilized": 1},
            {"rate_limit": {"primary_window": {"limit_window_seconds": 60}}}
        ]
    }"#;
    let meters = meters("codex", body);
    let rows: Vec<_> = meters
        .iter()
        .map(|m| {
            (
                m.label.as_str(),
                m.used.as_deref(),
                m.limit.as_deref(),
                m.unit.as_str(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("5h", Some("12.5"), Some("100"), "%"),
            ("1w", Some("40"), Some("100"), "%"),
            ("GPT-5 2d", Some("0"), Some("100"), "%"),
            ("Credits", Some("3"), Some("10"), "%"),
            ("Monthly", Some("1"), Some("2"), "%"),
        ]
    );
    assert_eq!(
        reset(&meters[0]).as_deref(),
        Some("2023-11-14T23:13:20+00:00")
    );
    assert_eq!(
        reset(&meters[1]).as_deref(),
        Some("2023-11-20T00:00:00+00:00")
    );
    assert_eq!(reset(&meters[2]), None);
    // Millisecond epochs are recognised by magnitude.
    assert_eq!(
        reset(&meters[3]).as_deref(),
        Some("2023-11-14T22:13:20+00:00")
    );
}

#[test]
fn codex_odd_window_lengths_fall_back_to_minutes_or_default() {
    let body = r#"{"rate_limit": {
        "primary_window": {"used_percent": 1, "limit_window_seconds": 5400},
        "secondary_window": {"used_percent": 2, "limit_window_seconds": "soon", "reset_after_seconds": 60}
    }}"#;
    let meters = meters("codex", body);
    assert_eq!(meters[0].label, "90m");
    assert_eq!(meters[1].label, "Weekly");
    assert!(meters[1].resets_at.is_some());
}

#[test]
fn copilot_quota_snapshots_and_legacy_premium_requests() {
    let body = r#"{
        "quota_reset_date": "2023-12-01",
        "quota_snapshots": {
            "chat": {"has_quota": true, "percent_remaining": 75},
            "premium_interactions": {"has_quota": true, "entitlement": 300, "remaining": 320},
            "completions": {"has_quota": true, "unlimited": true, "entitlement": 10},
            "code_review": {"has_quota": true, "entitlement": 50, "remaining": 20},
            "disabled": {"has_quota": false, "entitlement": 5}
        }
    }"#;
    let meters = meters("copilot", body);
    let rows: Vec<_> = meters
        .iter()
        .map(|m| {
            (
                m.label.as_str(),
                m.used.as_deref(),
                m.limit.as_deref(),
                m.unit.as_str(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("Premium requests", Some("0"), Some("300"), "requests"),
            ("Chat", Some("25"), Some("100"), "%"),
            ("code review", Some("30"), Some("50"), "requests"),
        ]
    );
    assert_eq!(
        reset(&meters[0]).as_deref(),
        Some("2023-12-01T00:00:00+00:00")
    );

    let legacy = r#"{"copilot": {"chat": {"premium_model_requests": {
        "total_premium_requests_used": 7, "total_premium_requests_included": 50,
        "usage_reset_date": "2023-12-01T00:00:00Z"}}}}"#;
    let meters = parse_snapshot("copilot", "oauth", legacy.as_bytes(), now())
        .unwrap()
        .meters;
    assert_eq!(meters.len(), 1);
    assert_eq!(meters[0].used.as_deref(), Some("7"));
    assert_eq!(meters[0].limit.as_deref(), Some("50"));
    assert_eq!(
        reset(&meters[0]).as_deref(),
        Some("2023-12-01T00:00:00+00:00")
    );
}
