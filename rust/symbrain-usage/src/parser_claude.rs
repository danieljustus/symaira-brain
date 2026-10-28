use super::{fmt_num, meter, number, obj, parse_time};
use crate::UsageSnapshot;
use serde_json::Value;

pub(super) fn parse_claude(v: &Value, s: &mut UsageSnapshot) {
    if let Some(v) = number(obj(v, "total_cost_usd")) {
        s.meters
            .push(meter("Spend (7d)", Some(fmt_num(v)), None, "USD", None));
        s.currency = Some("USD".into());
    }
    if let Some(v) = number(obj(v, "total_messages")) {
        s.meters.push(meter(
            "Messages (7d)",
            Some(fmt_num(v)),
            None,
            "requests",
            None,
        ));
    }
    s.currency = Some("USD".into());
    for key in ["five_hour", "seven_day"] {
        if let Some(window) = v.get(key) {
            append_window(key, window, s);
        }
    }
    // Scoped array entries supersede the old top-level model window, not the account quota.
    let mut scoped = std::collections::BTreeSet::new();
    if let Some(limits) = v.get("limits").and_then(Value::as_array) {
        for limit in limits {
            if limit.get("kind").and_then(Value::as_str) != Some("weekly_scoped") {
                continue;
            }
            let Some(name) = limit
                .pointer("/scope/model/display_name")
                .and_then(Value::as_str)
            else {
                continue;
            };
            if !["Fable", "Sonnet", "Opus"].contains(&name) || scoped.contains(name) {
                continue;
            }
            if let Some(used) = nonnegative(limit.get("percent")) {
                s.meters.push(meter(
                    name,
                    Some(fmt_num(used)),
                    Some("100".into()),
                    "%",
                    parse_time(limit.get("resets_at")),
                ));
                scoped.insert(name.to_string());
            }
        }
    }
    for (key, name) in [
        ("seven_day_sonnet", "Sonnet"),
        ("seven_day_opus", "Opus"),
        ("seven_day_fable", "Fable"),
    ] {
        if !scoped.contains(name)
            && let Some(window) = v.get(key)
        {
            append_window(name, window, s);
        }
    }
    if let Some(extra) = v.get("extra_usage") {
        let current = extra.get("is_enabled").is_some_and(|v| !v.is_null())
            || extra.get("used_credits").is_some_and(|v| !v.is_null())
            || extra.get("monthly_limit").is_some_and(|v| !v.is_null());
        let (used, limit) = if current {
            if extra.get("is_enabled").and_then(Value::as_bool) != Some(true) {
                return;
            }
            (
                nonnegative(extra.get("used_credits")).map(|v| v / 100.0),
                nonnegative(extra.get("monthly_limit"))
                    .filter(|v| *v > 0.0)
                    .map(|v| v / 100.0),
            )
        } else {
            (
                nonnegative(extra.get("used")),
                nonnegative(extra.get("limit")),
            )
        };
        if used.is_some() || limit.is_some() {
            s.meters.push(meter(
                "Extra usage",
                used.map(fmt_num),
                limit.map(fmt_num),
                "USD",
                None,
            ));
        }
    }
}

fn nonnegative(v: Option<&Value>) -> Option<f64> {
    // Match Go's numeric JSON fields; never emit NaN, infinity or negative sentinels.
    v.and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v >= 0.0)
}

fn append_window(label: &str, window: &Value, s: &mut UsageSnapshot) {
    let (used, limit) = if window.get("utilization").is_some_and(|v| !v.is_null()) {
        (nonnegative(window.get("utilization")), Some(100.0))
    } else {
        (
            nonnegative(window.get("utilized")),
            nonnegative(window.get("limit")).filter(|v| *v > 0.0),
        )
    };
    if limit.is_some() && (window.get("utilization").is_none_or(Value::is_null) || used.is_some()) {
        s.meters.push(meter(
            label,
            used.map(fmt_num),
            limit.map(fmt_num),
            "%",
            parse_time(window.get("resets_at")),
        ));
    }
}
