use super::{UsageError, UsageSnapshot, fmt_num, meter, number, parse_time, percent, text};
use serde_json::Value;

pub(super) fn parse_cursor(v: &Value, s: &mut UsageSnapshot) {
    let plan = v
        .pointer("/individualUsage/plan")
        .or_else(|| v.pointer("/individual_usage/plan"));
    let reset = parse_time(v.get("billingCycleEnd"));
    if let Some(plan) =
        plan.filter(|plan| plan.get("enabled").and_then(Value::as_bool) != Some(false))
    {
        if let Some(p) = number(plan.get("totalPercentUsed")) {
            s.meters.push(meter(
                "Plan usage",
                Some(fmt_num(percent(p * 100.0))),
                Some("100".into()),
                "%",
                reset,
            ));
        } else if let (Some(used), Some(limit)) =
            (number(plan.get("used")), number(plan.get("limit")))
        {
            s.meters.push(meter(
                "Plan usage",
                Some(fmt_num(used)),
                Some(fmt_num(limit)),
                "USD",
                reset,
            ));
        }
        for (key, label) in [
            ("autoPercentUsed", "Auto usage"),
            ("apiPercentUsed", "API usage"),
        ] {
            if let Some(p) = number(plan.get(key)) {
                s.meters.push(meter(
                    label,
                    Some(fmt_num(percent(p * 100.0))),
                    Some("100".into()),
                    "%",
                    reset,
                ));
            }
        }
    }
    if let Some(used) = v
        .pointer("/individualUsage/onDemand")
        .filter(|on| on.get("enabled").and_then(Value::as_bool) != Some(false))
        .and_then(|on| number(on.get("used")))
    {
        let on = v
            .pointer("/individualUsage/onDemand")
            .expect("filtered above");
        s.meters.push(meter(
            "On-demand usage",
            Some(fmt_num(used)),
            number(on.get("limit")).filter(|x| *x > 0.0).map(fmt_num),
            "USD",
            reset,
        ));
    }
}

pub(super) fn parse_kimi(v: &Value, s: &mut UsageSnapshot) {
    if let Some(usage) = v.get("usage") {
        kimi_detail(usage, "Weekly quota", s);
    }
    if let Some(limits) = v.get("limits").and_then(Value::as_array) {
        for limit in limits {
            kimi_detail(
                limit.get("detail").unwrap_or(&Value::Null),
                &kimi_window_label(limit.get("window")),
                s,
            );
        }
    }
    if let Some(usages) = v.get("usages").and_then(Value::as_array) {
        let item = usages
            .iter()
            .find(|x| x.get("scope").and_then(Value::as_str) == Some("FEATURE_CODING"))
            .or_else(|| usages.first());
        if let Some(item) = item {
            kimi_detail(
                item.get("detail").unwrap_or(&Value::Null),
                "Weekly quota",
                s,
            );
            if let Some(limits) = item.get("limits").and_then(Value::as_array) {
                for limit in limits {
                    kimi_detail(
                        limit.get("detail").unwrap_or(&Value::Null),
                        &kimi_window_label(limit.get("window")),
                        s,
                    );
                }
            }
        }
    }
}
fn kimi_detail(v: &Value, label: &str, s: &mut UsageSnapshot) {
    let (Some(used), Some(limit)) = (number(v.get("used")), number(v.get("limit"))) else {
        return;
    };
    if limit <= 0.0 {
        return;
    }
    s.meters.push(meter(
        label,
        Some(fmt_num(used)),
        Some(fmt_num(limit)),
        "requests",
        parse_time(v.get("resetTime")),
    ));
}
fn kimi_window_label(v: Option<&Value>) -> String {
    let Some(n) = v.and_then(|x| number(x.get("duration"))) else {
        return "Rate limit window".into();
    };
    let Some(n) = super::whole_f64(n) else {
        return "Rate limit window".into();
    };
    if n % 60 == 0 {
        format!("{}h window", n / 60)
    } else {
        format!("{n}min window")
    }
}

pub(super) fn parse_moonshot(v: &Value, s: &mut UsageSnapshot) {
    let currency = s.source.strip_prefix("cn:").map_or("USD", |_| "CNY");
    if let Some(v) = number(v.get("available_balance")) {
        s.balance = Some(fmt_num(v));
    }
    if let Some(v) = number(v.get("cash_balance")) {
        s.meters.push(meter(
            "Cash balance",
            Some(fmt_num(v)),
            None,
            currency,
            None,
        ));
    }
    if let Some(v) = number(v.get("voucher_balance")) {
        s.meters.push(meter(
            "Voucher balance",
            Some(fmt_num(v)),
            None,
            currency,
            None,
        ));
    }
    s.currency = Some(currency.into());
}
pub(super) fn parse_nous(v: &Value, s: &mut UsageSnapshot) {
    if let Some(access) = v.get("paid_service_access") {
        for (key, label) in [
            ("subscription_credits_remaining", "Subscription credits"),
            ("purchased_credits_remaining", "Purchased credits"),
        ] {
            if let Some(v) = number(access.get(key)) {
                s.meters
                    .push(meter(label, Some(fmt_num(v)), None, "credits", None));
            }
        }
        if let Some(v) = number(access.get("total_usable_credits")) {
            s.balance = Some(fmt_num(v));
        }
    }
    if let Some(sub) = v.get("subscription") {
        if let Some(v) = number(sub.get("credits_remaining")) {
            s.meters.push(meter(
                "Plan credits remaining",
                Some(fmt_num(v)),
                number(sub.get("monthly_credits")).map(fmt_num),
                "credits",
                parse_time(sub.get("current_period_end")),
            ));
        }
        if let Some(v) = number(sub.get("rollover_credits")) {
            s.meters.push(meter(
                "Rollover credits",
                Some(fmt_num(v)),
                None,
                "credits",
                None,
            ));
        }
    }
}
pub(super) fn parse_openrouter(v: &Value, s: &mut UsageSnapshot) -> Result<(), UsageError> {
    let data = v.get("data").ok_or_else(|| {
        UsageError::parse("openrouter", "response contained no usable usage fields")
    })?;
    let usage = number(data.get("usage"));
    if let Some(limit) = number(data.get("limit")) {
        s.meters.push(meter(
            "Key limit",
            usage.map(fmt_num),
            Some(fmt_num(limit)),
            "USD",
            parse_time(data.pointer("/usage_period/end_time")),
        ));
    } else if let Some(usage) = usage {
        s.meters.push(meter(
            "Spend",
            Some(fmt_num(usage)),
            None,
            "USD",
            parse_time(data.pointer("/usage_period/end_time")),
        ));
    }
    if let Some(req) = number(data.pointer("/rate_limit/requests")).filter(|x| *x >= 0.0) {
        s.meters.push(meter(
            "Requests",
            None,
            Some(fmt_num(req)),
            "requests",
            None,
        ));
    }
    if let (Some(total), Some(used)) = (number(data.get("total_credits")), usage) {
        s.balance = Some(fmt_num(total - used));
    }
    s.currency = Some("USD".into());
    Ok(())
}
pub(super) fn parse_opencode(v: &Value, s: &mut UsageSnapshot) -> Result<(), UsageError> {
    let mut windows = Vec::new();
    collect_windows(v, &mut windows);
    if windows.is_empty() {
        return Err(UsageError::parse("opencode", "missing usage fields"));
    }
    for (i, (p, reset)) in windows.into_iter().take(2).enumerate() {
        let label = if i == 0 { "5h window" } else { "This week" };
        let reset = reset.and_then(super::whole_f64).and_then(|x| {
            s.fetched_at
                .checked_add_signed(chrono::Duration::seconds(x))
        });
        s.meters.push(meter(
            label,
            Some(fmt_num(percent(p))),
            Some("100".into()),
            "%",
            reset,
        ));
    }
    Ok(())
}
fn collect_windows(v: &Value, out: &mut Vec<(f64, Option<f64>)>) {
    if let Some(map) = v.as_object() {
        for key in [
            "rollingUsage",
            "weeklyUsage",
            "usage",
            "billing",
            "data",
            "result",
        ] {
            if let Some(child) = map.get(key) {
                collect_windows(child, out);
            }
        }
        if let Some(p) = [
            "usagePercent",
            "usedPercent",
            "percentUsed",
            "percent",
            "usage_percent",
            "utilization",
            "usage",
        ]
        .iter()
        .find_map(|k| number(map.get(*k)))
        {
            let r = [
                "resetInSec",
                "resetInSeconds",
                "reset_sec",
                "resetsInSec",
                "resetIn",
                "resetSec",
            ]
            .iter()
            .find_map(|k| number(map.get(*k)));
            out.push((p, r));
        }
    } else if let Some(array) = v.as_array() {
        for child in array {
            collect_windows(child, out);
        }
    }
}
pub(super) fn parse_antigravity(v: &Value, s: &mut UsageSnapshot) -> Result<(), UsageError> {
    let groups = v
        .pointer("/response/groups")
        .or_else(|| v.pointer("/summary/groups"))
        .or_else(|| v.get("groups"));
    if let Some(groups) = groups.and_then(Value::as_array) {
        for group in groups {
            let name = text(group.get("displayName")).unwrap_or("Quota");
            if let Some(buckets) = group.get("buckets").and_then(Value::as_array) {
                for bucket in buckets {
                    if let Some(rem) =
                        number(bucket.get("remainingFraction")).filter(|x| (0.0..=1.0).contains(x))
                    {
                        let label = text(bucket.get("displayName")).unwrap_or(name);
                        let used = Some(fmt_num(percent(((1.0 - rem) * 100.0).round())));
                        s.meters.push(meter(
                            format!("{name} — {label}"),
                            used,
                            Some("100".into()),
                            "%",
                            parse_time(bucket.get("resetTime")),
                        ));
                    }
                }
            }
        }
    } else if let Some(configs) = v
        .pointer("/userStatus/cascadeModelConfigData/clientModelConfigs")
        .or_else(|| v.get("clientModelConfigs"))
        .and_then(Value::as_array)
    {
        for config in configs {
            if let Some(rem) = number(config.pointer("/quotaInfo/remainingFraction"))
                .filter(|x| (0.0..=1.0).contains(x))
            {
                let label = text(config.get("label"))
                    .filter(|x| !x.is_empty())
                    .or_else(|| text(config.pointer("/modelOrAlias/model")))
                    .unwrap_or("Quota");
                s.meters.push(meter(
                    label,
                    Some(fmt_num(((1.0 - rem) * 100.0).round())),
                    Some("100".into()),
                    "%",
                    parse_time(config.pointer("/quotaInfo/resetTime")),
                ));
            }
        }
    } else {
        return Err(UsageError::parse("antigravity", "missing quota fields"));
    }
    Ok(())
}
