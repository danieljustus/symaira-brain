// Offline fixtures for organization-billed Copilot seats. The response shapes
// follow GitHub's documented REST schema (usageItems[].product/unitType/
// grossQuantity) and the OpenUsage reference mapper; no real organization
// response was available to record (see #1001).
use crate::providers::copilot_from_resolved;
use crate::{FixtureTransport, Response, Service};
use std::collections::BTreeMap;
use std::sync::Arc;

const TOKEN: &str = "ghu_FIXTURE_TOKEN_SECRET";

const ORG_SEAT: &str = r#"{"copilot_plan":"business","token_based_billing":true,
 "quota_snapshots":{"premium_interactions":{"entitlement":0,"remaining":0,"unlimited":false,"has_quota":false}}}"#;
const PERSONAL: &str = r#"{"copilot_plan":"individual","quota_reset_date_utc":"2026-10-01T00:00:00.000Z",
 "quota_snapshots":{"premium_interactions":{"entitlement":300,"remaining":255,"percent_remaining":85.0,"unlimited":false,"has_quota":true}}}"#;
const ORGS: &str = r#"[{"login":"acme-inc","id":1},{"login":"second-org","id":2}]"#;
const SUMMARY: &str = r#"{"timePeriod":{"year":2026,"month":11},"organization":"acme-inc","usageItems":[
 {"product":"Copilot","sku":"Copilot AI Credits","unitType":"ai-credits","grossQuantity":1200.5,"grossAmount":12.0,"netAmount":2.0},
 {"product":"copilot","sku":"Copilot Agent","unitType":"ai-units","grossQuantity":"100","netAmount":1.0},
 {"product":"Copilot","sku":"Copilot Business seat","unitType":"user-months","grossQuantity":50,"netAmount":950},
 {"product":"Actions","sku":"Linux","unitType":"ai-credits","grossQuantity":9999}]}"#;
const NO_COPILOT: &str =
    r#"{"usageItems":[{"product":"Actions","unitType":"minutes","grossQuantity":5}]}"#;

#[allow(clippy::unnecessary_wraps)] // sequence elements are Results
fn ok(body: &str) -> Result<Response, String> {
    status(200, body)
}

#[allow(clippy::unnecessary_wraps)] // sequence elements are Results
fn status(code: u16, body: &str) -> Result<Response, String> {
    Ok(Response {
        status: code,
        body: body.as_bytes().to_vec(),
        headers: BTreeMap::new(),
    })
}

/// Runs one report; returns the provider row JSON and the requested URLs.
fn run(sequence: Vec<Result<Response, String>>) -> (serde_json::Value, Vec<String>) {
    let transport =
        FixtureTransport::with_sequences(BTreeMap::from([("copilot".into(), sequence)]));
    let provider = copilot_from_resolved(Some(("env".into(), TOKEN.into())), None);
    let report = Service::with_transport(vec![provider], Arc::new(transport.clone())).report();
    let urls = transport.requests().into_iter().map(|r| r.url).collect();
    let json = serde_json::to_value(&report.providers[0]).unwrap();
    assert!(
        !json.to_string().contains("SECRET"),
        "credential leaked: {json}"
    );
    (json, urls)
}

#[test]
fn owner_sees_the_organization_credit_total_without_any_cost() {
    let (row, urls) = run(vec![ok(ORG_SEAT), ok(ORGS), ok(SUMMARY)]);
    assert_eq!(
        urls,
        [
            "https://api.github.com/copilot_internal/user",
            "https://api.github.com/user/orgs?per_page=100",
            "https://api.github.com/orgs/acme-inc/settings/billing/usage/summary",
        ]
    );
    assert!(row.get("error").is_none(), "{row}");
    let snapshot = &row["snapshot"];
    // 1200.5 + 100; seat-fee item and the other product are excluded.
    assert_eq!(
        snapshot["meters"],
        serde_json::json!([{"label":"Organization credits","used":"1300.5","unit":"credits"}])
    );
    assert_eq!(snapshot["org_billing"]["state"], "available");
    assert_eq!(snapshot["org_billing"]["organization"], "acme-inc");
    assert!(!snapshot.to_string().contains("netAmount"));
    assert!(snapshot.get("balance").is_none() && snapshot.get("currency").is_none());
}

#[test]
fn member_403_is_a_documented_non_fatal_state() {
    let (row, urls) = run(vec![
        ok(ORG_SEAT),
        ok(ORGS),
        status(403, "{}"),
        status(403, "{}"),
    ]);
    assert_eq!(urls.len(), 4, "every organization is probed");
    assert!(
        row.get("error").is_none(),
        "403 must not fail the row: {row}"
    );
    let billing = &row["snapshot"]["org_billing"];
    assert_eq!(billing["state"], "forbidden");
    assert!(
        billing["detail"]
            .as_str()
            .unwrap()
            .contains("owners and billing managers")
    );
    assert_eq!(row["snapshot"]["meters"], serde_json::json!([]));
    assert!(billing.get("organization").is_none());
}

#[test]
fn a_later_organization_can_supply_the_summary() {
    let (row, _) = run(vec![ok(ORG_SEAT), ok(ORGS), status(403, "{}"), ok(SUMMARY)]);
    assert_eq!(row["snapshot"]["org_billing"]["organization"], "second-org");
}

#[test]
fn no_organization_is_reported_not_failed() {
    let (row, urls) = run(vec![ok(ORG_SEAT), ok("[]")]);
    assert_eq!(urls.len(), 2);
    assert!(row.get("error").is_none());
    assert_eq!(row["snapshot"]["org_billing"]["state"], "no_organization");
}

#[test]
fn organization_list_denied_and_outages_stay_non_fatal() {
    let (row, _) = run(vec![ok(ORG_SEAT), status(403, "{}")]);
    assert_eq!(row["snapshot"]["org_billing"]["state"], "forbidden");
    let (row, _) = run(vec![ok(ORG_SEAT), Err("connection reset".into())]);
    assert_eq!(row["snapshot"]["org_billing"]["state"], "unavailable");
    assert!(!row.to_string().contains("connection reset"));
    let (row, _) = run(vec![
        ok(ORG_SEAT),
        ok(ORGS),
        status(503, ""),
        ok(NO_COPILOT),
    ]);
    assert_eq!(row["snapshot"]["org_billing"]["state"], "unavailable");
    let (row, _) = run(vec![ok(ORG_SEAT), ok(ORGS), ok(NO_COPILOT), ok(NO_COPILOT)]);
    assert_eq!(row["snapshot"]["org_billing"]["state"], "no_copilot_usage");
}

#[test]
fn organization_slugs_cannot_alter_the_request_path() {
    let orgs = r#"[{"login":"../x"},{"login":"a/b"},{"login":"ok-org"}]"#;
    let (_, urls) = run(vec![ok(ORG_SEAT), ok(orgs), ok(NO_COPILOT)]);
    assert_eq!(urls.len(), 3);
    assert!(urls[2].ends_with("/orgs/ok-org/settings/billing/usage/summary"));
}

#[test]
fn personal_seats_make_no_organization_request_and_keep_their_shape() {
    let (row, urls) = run(vec![ok(PERSONAL)]);
    assert_eq!(urls, ["https://api.github.com/copilot_internal/user"]);
    assert!(row["snapshot"].get("org_billing").is_none());
    assert_eq!(row["snapshot"]["meters"][0]["label"], "Premium requests");
}

#[test]
fn user_endpoint_failures_never_reach_the_organization_lookup() {
    let (row, urls) = run(vec![status(401, "{}")]);
    assert_eq!(urls.len(), 1);
    assert!(row["error"].as_str().is_some());
    // Unmarked empty payloads stay the frozen payload error.
    let (row, urls) = run(vec![ok(r#"{"quota_snapshots":{}}"#)]);
    assert_eq!(urls.len(), 1);
    assert!(
        row["error"]
            .as_str()
            .unwrap()
            .contains("no usable usage fields")
    );
}

#[test]
fn enterprise_hosts_use_their_own_api_root() {
    assert_eq!(super::api_base(None), "https://api.github.com");
    assert_eq!(
        super::api_base(Some("github.com")),
        "https://api.github.com"
    );
    assert_eq!(
        super::api_base(Some("ghe.example.com")),
        "https://ghe.example.com/api/v3"
    );
}
