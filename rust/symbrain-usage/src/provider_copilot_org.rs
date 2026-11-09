//! Copilot seats billed to an organization (#1001).
//!
//! The user-scoped quota never carries the usage of such a seat
//! (`token_based_billing` without a per-seat quota). Its usage lives in the
//! organization's billing summary, which only owners and billing managers may
//! read. This module looks it up with the credential the Copilot fetch already
//! uses: `GET /user/orgs`, then `GET /orgs/{org}/settings/billing/usage/summary`.
//! Usage only: no cost or price is read or computed, and no new token scope is
//! requested. Any failure of the lookup is reported in `org_billing`, never as
//! a failed provider, so personal seats and the rest of the report are unchanged.
use super::provider_fetch::fetch_one_response;
use super::{Provider, UsageError, request_for};
use crate::parser::{fmt_num, parse_snapshot};
use crate::transport::{Cancellation, Request, Response, Transport};
use crate::{OrgBilling, UsageMeter, UsageSnapshot};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Bounds the probe requests of one report; the provider timeout is short.
const MAX_ORGANIZATIONS: usize = 10;
const FORBIDDEN_DETAIL: &str = "Organization billing is readable only by organization owners and billing managers (HTTP 403); no organization usage is shown";

pub(super) fn fetch(
    provider: &Provider,
    transport: &Arc<dyn Transport>,
    cancel: &Cancellation,
) -> Result<UsageSnapshot, UsageError> {
    let request = request_for(provider, "api", provider.credential(), None);
    let response = fetch_one_response(provider, transport, cancel, request)?;
    let now = Utc::now();
    match parse_snapshot(&provider.id, "api", &response.body, now) {
        // Only an organization-billed seat: no per-seat meter and the explicit marker.
        Err(_) if token_based_billing(&response.body) => {
            Ok(org_snapshot(provider, transport, cancel, now))
        }
        other => other,
    }
}

fn token_based_billing(body: &[u8]) -> bool {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("token_based_billing").and_then(Value::as_bool))
        == Some(true)
}

fn org_snapshot(
    provider: &Provider,
    transport: &Arc<dyn Transport>,
    cancel: &Cancellation,
    now: DateTime<Utc>,
) -> UsageSnapshot {
    let (meter, billing) = lookup(provider, transport, cancel);
    UsageSnapshot {
        provider_id: provider.id.clone(),
        meters: meter.into_iter().collect(),
        fetched_at: now,
        source: "api".into(),
        org_billing: Some(billing),
        ..UsageSnapshot::default()
    }
}

fn status(state: &str, detail: &str, organization: Option<String>) -> OrgBilling {
    OrgBilling {
        state: state.into(),
        detail: detail.into(),
        organization,
    }
}

fn api_base(host: Option<&str>) -> String {
    match host.filter(|h| !h.is_empty() && *h != "github.com") {
        None => "https://api.github.com".into(),
        Some(host) => format!("https://{host}/api/v3"),
    }
}

fn get(
    provider: &Provider,
    transport: &Arc<dyn Transport>,
    cancel: &Cancellation,
    path: &str,
) -> Result<Response, String> {
    let url = format!("{}{path}", api_base(provider.enterprise_host.as_deref()));
    let headers = BTreeMap::from([
        (
            "Accept".to_owned(),
            "application/vnd.github+json".to_owned(),
        ),
        (
            "Authorization".to_owned(),
            format!("Bearer {}", provider.credential()),
        ),
        ("X-GitHub-Api-Version".to_owned(), "2022-11-28".to_owned()),
    ]);
    transport.request_with_cancel(
        Request {
            provider_id: provider.id.clone(),
            method: "GET".into(),
            url,
            headers,
            header_bytes: BTreeMap::new(),
            body: None,
        },
        cancel,
    )
}

fn lookup(
    provider: &Provider,
    transport: &Arc<dyn Transport>,
    cancel: &Cancellation,
) -> (Option<UsageMeter>, OrgBilling) {
    let unavailable = |detail: &str| (None, status("unavailable", detail, None));
    let list = match get(provider, transport, cancel, "/user/orgs?per_page=100") {
        Ok(r) if r.status == 200 => r,
        // Editor-plugin tokens may lack `read:org`; expected, not an error.
        Ok(r) if r.status == 401 || r.status == 403 => {
            return (None, status("forbidden", FORBIDDEN_DETAIL, None));
        }
        Ok(r) => {
            return unavailable(&format!("organization list returned HTTP {}", r.status));
        }
        Err(_) => return unavailable("organization list request failed"),
    };
    let logins = org_logins(&list.body);
    if logins.is_empty() {
        return (
            None,
            status(
                "no_organization",
                "The account belongs to no organization, so there is no organization billing to read",
                None,
            ),
        );
    }
    let (mut forbidden, mut failed) = (false, false);
    for login in logins {
        let path = format!("/orgs/{login}/settings/billing/usage/summary");
        match get(provider, transport, cancel, &path) {
            Ok(r) if r.status == 200 => {
                if let Some(credits) = copilot_credits(&r.body) {
                    let meter = UsageMeter {
                        label: "Organization credits".into(),
                        used: Some(fmt_num(credits)),
                        limit: None,
                        unit: "credits".into(),
                        resets_at: None,
                    };
                    let detail =
                        "Month-to-date Copilot credits of the whole organization, not of this seat";
                    return (Some(meter), status("available", detail, Some(login)));
                }
            }
            Ok(r) if r.status == 403 => forbidden = true,
            Ok(r) if r.status == 429 || r.status >= 500 => failed = true,
            Ok(_) => {}
            Err(_) => failed = true,
        }
    }
    if forbidden {
        (None, status("forbidden", FORBIDDEN_DETAIL, None))
    } else if failed {
        unavailable("organization billing summary could not be read")
    } else {
        (
            None,
            status(
                "no_copilot_usage",
                "No organization billing summary carries Copilot credit usage",
                None,
            ),
        )
    }
}

/// Organization slugs of a `/user/orgs` body; slugs that could alter the
/// request path are dropped.
fn org_logins(body: &[u8]) -> Vec<String> {
    let Ok(Value::Array(items)) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| item.get("login")?.as_str())
        .map(str::trim)
        .filter(|login| {
            !login.is_empty()
                && login
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
        .take(MAX_ORGANIZATIONS)
        .map(str::to_owned)
        .collect()
}

/// Sum of `grossQuantity` over the Copilot credit items of a billing summary,
/// or `None` when the summary has none (the organization does not use Copilot
/// credits). Seat-fee items with other units never enter the total.
fn copilot_credits(body: &[u8]) -> Option<f64> {
    let root: Value = serde_json::from_slice(body).ok()?;
    let mut items = root
        .get("usageItems")?
        .as_array()?
        .iter()
        .filter(|item| {
            let text = |key: &str| item.get(key).and_then(Value::as_str).map(str::trim);
            text("product").is_some_and(|p| p.eq_ignore_ascii_case("copilot"))
                && text("unitType").is_some_and(|u| {
                    u.eq_ignore_ascii_case("ai-units") || u.eq_ignore_ascii_case("ai-credits")
                })
        })
        .peekable();
    items.peek()?;
    Some(
        items
            .map(|item| match item.get("grossQuantity") {
                Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
                Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
                _ => 0.0,
            })
            .map(|q| q.max(0.0))
            .sum(),
    )
}

#[cfg(test)]
#[path = "provider_copilot_org_tests.rs"]
mod tests;
