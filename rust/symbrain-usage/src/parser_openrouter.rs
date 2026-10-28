use super::{fmt_num, meter};
use crate::{UsageError, UsageSnapshot};
use serde_json::Value;

fn amount(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v >= 0.0)
}

pub(super) fn credits(v: &Value, s: &mut UsageSnapshot) -> Result<(), UsageError> {
    let data = &v["data"];
    // Go's typed decoder rejects wrong JSON types before validating required amounts.
    if (!v.is_null() && !v.is_object())
        || (!data.is_null() && !data.is_object())
        || ["total_credits", "total_usage"].iter().any(|field| {
            data.get(field)
                .is_some_and(|value| !value.is_null() && !value.is_number())
        })
    {
        return Err(UsageError::parse("openrouter", "response is not JSON"));
    }
    let (Some(credits), Some(usage)) = (
        amount(data.get("total_credits")),
        amount(data.get("total_usage")),
    ) else {
        return Err(UsageError::payload(
            "openrouter",
            "missing or invalid account credits",
        ));
    };
    s.balance = Some(fmt_num(credits - usage));
    s.currency = Some("USD".into());
    s.source = "api".into();
    s.meters.push(meter(
        "Lifetime credits",
        Some(fmt_num(credits)),
        None,
        "USD",
        None,
    ));
    s.meters.push(meter(
        "Lifetime spend",
        Some(fmt_num(usage)),
        None,
        "USD",
        None,
    ));
    Ok(())
}

pub(crate) fn enrich(body: &[u8], s: &mut UsageSnapshot) {
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return;
    };
    let Some(k) = v.get("data").filter(|v| v.is_object()) else {
        return;
    };
    // Match Go's typed supplemental decoder: a malformed known field drops enrichment only.
    for field in [
        "usage_daily",
        "usage_weekly",
        "usage_monthly",
        "limit",
        "limit_remaining",
    ] {
        if k.get(field)
            .is_some_and(|v| !v.is_null() && v.as_f64().is_none())
        {
            return;
        }
    }
    if let Some(rate) = k.get("rate_limit").filter(|v| !v.is_null())
        && (!rate.is_object()
            || rate
                .get("requests")
                .is_some_and(|v| !v.is_null() && v.as_i64().is_none())
            || rate
                .get("interval")
                .is_some_and(|v| !v.is_null() && !v.is_string()))
    {
        return;
    }
    for (field, label) in [
        ("usage_daily", "Today"),
        ("usage_weekly", "This Week"),
        ("usage_monthly", "This Month"),
    ] {
        if let Some(used) = amount(k.get(field)) {
            s.meters
                .push(meter(label, Some(fmt_num(used)), None, "USD", None));
        }
    }
    if let Some(limit) = amount(k.get("limit")) {
        let used = amount(k.get("limit_remaining"))
            .filter(|r| *r <= limit)
            .map(|r| fmt_num(limit - r));
        s.meters
            .push(meter("Key limit", used, Some(fmt_num(limit)), "USD", None));
    }
    if let Some(requests) = k
        .pointer("/rate_limit/requests")
        .and_then(Value::as_i64)
        .filter(|v| *v >= 0)
    {
        s.meters.push(meter(
            "Requests",
            None,
            Some(requests.to_string()),
            "requests",
            None,
        ));
    }
}
