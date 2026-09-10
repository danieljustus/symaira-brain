use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::{UsageError, UsageMeter, UsageSnapshot};

#[path = "parser_extra.rs"]
mod parser_extra;

pub(crate) fn parse_snapshot(
    id: &str,
    source: &str,
    body: &[u8],
    now: DateTime<Utc>,
) -> Result<UsageSnapshot, UsageError> {
    if body.is_empty() {
        return Err(UsageError::parse(id, "empty response"));
    }
    let value: Value =
        serde_json::from_slice(body).map_err(|_| UsageError::parse(id, "response is not JSON"))?;
    let mut snapshot = UsageSnapshot {
        provider_id: id.to_string(),
        meters: Vec::new(),
        balance: None,
        currency: None,
        fetched_at: now,
        source: source.to_string(),
    };
    match id {
        "claude" => {
            parse_claude(&value, &mut snapshot);
            Ok(())
        }
        "codex" => {
            parse_codex(&value, &mut snapshot);
            Ok(())
        }
        "copilot" => {
            parse_copilot(&value, &mut snapshot);
            Ok(())
        }
        "cursor" => {
            parser_extra::parse_cursor(&value, &mut snapshot);
            Ok(())
        }
        "kimi" => {
            parser_extra::parse_kimi(&value, &mut snapshot);
            Ok(())
        }
        "moonshot" => {
            parser_extra::parse_moonshot(&value, &mut snapshot);
            Ok(())
        }
        "nous" => {
            parser_extra::parse_nous(&value, &mut snapshot);
            Ok(())
        }
        "opencode" => parser_extra::parse_opencode(&value, &mut snapshot),
        "openrouter" => parser_extra::parse_openrouter(&value, &mut snapshot),
        "antigravity" => parser_extra::parse_antigravity(&value, &mut snapshot),
        _ => Err(UsageError::parse(id, "unknown provider")),
    }?;
    if snapshot.meters.is_empty() && snapshot.balance.is_none() {
        return Err(UsageError::parse(
            id,
            if id == "opencode" {
                "missing usage fields"
            } else {
                "response contained no usable usage fields"
            },
        ));
    }
    Ok(snapshot)
}

fn number(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    }
}
fn text(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str)
}
fn obj<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key)
}
fn fmt_num(v: f64) -> String {
    if v == -0.0 {
        "0".into()
    } else {
        format!("{v}")
    }
}
fn meter(
    label: impl Into<String>,
    used: Option<String>,
    limit: Option<String>,
    unit: &str,
    reset: Option<DateTime<Utc>>,
) -> UsageMeter {
    UsageMeter {
        label: label.into(),
        used,
        limit,
        unit: unit.into(),
        resets_at: reset,
    }
}
fn parse_time(v: Option<&Value>) -> Option<DateTime<Utc>> {
    if let Some(s) = text(v) {
        if let Ok(time) = DateTime::parse_from_rfc3339(s) {
            return Some(time.with_timezone(&Utc));
        }
        if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            return date
                .and_hms_opt(0, 0, 0)
                .map(|value| DateTime::<Utc>::from_naive_utc_and_offset(value, Utc));
        }
    }
    number(v).and_then(|value| {
        let millis = value.abs() >= 1_000_000_000_000.0;
        let seconds = if millis { value / 1000.0 } else { value };
        whole_f64(seconds).and_then(|value| DateTime::from_timestamp(value, 0))
    })
}
fn whole_f64(value: f64) -> Option<i64> {
    if !value.is_finite() {
        return None;
    }
    value.trunc().to_string().parse().ok()
}
fn percent(v: f64) -> f64 {
    v.clamp(0.0, 100.0)
}

fn parse_claude(v: &Value, s: &mut UsageSnapshot) {
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
    for (key, label) in [("five_hour", "five_hour"), ("seven_day", "seven_day")] {
        let window = obj(v, key).and_then(|window| {
            Some((
                window,
                number(window.get("limit"))?,
                number(window.get("utilized").or_else(|| window.get("used")))?,
            ))
        });
        if let Some((window, limit, used)) = window {
            s.meters.push(meter(
                label,
                Some(fmt_num(used)),
                Some(fmt_num(limit)),
                "%",
                parse_time(window.get("resets_at")),
            ));
        }
    }
    if let Some(extra) = obj(v, "extra_usage") {
        let used = number(extra.get("used")).map(fmt_num);
        let limit = number(extra.get("limit")).map(fmt_num);
        if used.is_some() || limit.is_some() {
            s.meters
                .push(meter("Extra usage", used, limit, "USD", None));
            s.currency = Some("USD".into());
        }
    }
}

fn parse_codex(v: &Value, s: &mut UsageSnapshot) {
    if let Some(rate) = obj(v, "rate_limit") {
        parse_rate(rate, s, "");
    }
    if let Some(extras) = obj(v, "additional_rate_limits").and_then(Value::as_array) {
        for extra in extras {
            let prefix = text(extra.get("limit_name")).unwrap_or("Codex");
            if let Some(rate) = extra.get("rate_limit") {
                parse_rate(rate, s, prefix);
            } else if let (Some(used), Some(limit)) =
                (number(extra.get("utilized")), number(extra.get("limit")))
            {
                s.meters.push(meter(
                    text(extra.get("title"))
                        .or_else(|| text(extra.get("window")))
                        .unwrap_or("Codex"),
                    Some(fmt_num(used)),
                    Some(fmt_num(limit)),
                    "%",
                    parse_time(extra.get("reset_date")),
                ));
            }
        }
    }
}
fn parse_rate(v: &Value, s: &mut UsageSnapshot, prefix: &str) {
    for (key, fallback) in [
        ("primary_window", "Session"),
        ("secondary_window", "Weekly"),
    ] {
        let Some(w) = v.get(key) else { continue };
        let used = number(w.get("utilized").or_else(|| w.get("used_percent")));
        let Some(used) = used else { continue };
        let limit = number(w.get("limit")).unwrap_or(100.0);
        let label = text(w.get("window")).map_or_else(
            || window_label(w.get("limit_window_seconds"), fallback),
            ToOwned::to_owned,
        );
        let label = if prefix.is_empty() {
            label
        } else {
            format!("{prefix} {label}")
        };
        let reset = parse_time(w.get("reset_date"))
            .or_else(|| {
                number(w.get("reset_at"))
                    .and_then(whole_f64)
                    .and_then(|x| DateTime::from_timestamp(x, 0))
            })
            .or_else(|| {
                number(w.get("reset_after_seconds"))
                    .and_then(whole_f64)
                    .and_then(|x| Utc::now().checked_add_signed(chrono::Duration::seconds(x)))
            });
        s.meters.push(meter(
            label,
            Some(fmt_num(used)),
            Some(fmt_num(limit)),
            "%",
            reset,
        ));
    }
}
fn window_label(v: Option<&Value>, fallback: &str) -> String {
    let Some(v) = number(v) else {
        return fallback.into();
    };
    let Some(n) = whole_f64(v) else {
        return fallback.into();
    };
    if n % 604_800 == 0 {
        format!("{}w", n / 604_800)
    } else if n % 86_400 == 0 {
        format!("{}d", n / 86_400)
    } else if n % 3_600 == 0 {
        format!("{}h", n / 3_600)
    } else {
        format!("{}m", n / 60)
    }
}

fn parse_copilot(v: &Value, s: &mut UsageSnapshot) {
    if let Some(map) = obj(v, "quota_snapshots").and_then(Value::as_object) {
        let reset = parse_time(v.get("quota_reset_date_utc")).or_else(|| {
            text(v.get("quota_reset_date"))
                .and_then(|x| chrono::NaiveDate::parse_from_str(x, "%Y-%m-%d").ok())
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|x| DateTime::<Utc>::from_naive_utc_and_offset(x, Utc))
        });
        for id in ["premium_interactions", "chat", "completions"]
            .into_iter()
            .chain(
                map.keys()
                    .map(String::as_str)
                    .filter(|id| !["premium_interactions", "chat", "completions"].contains(id)),
            )
        {
            let Some(q) = map.get(id) else { continue };
            if q.get("unlimited").and_then(Value::as_bool).unwrap_or(false)
                || !q.get("has_quota").and_then(Value::as_bool).unwrap_or(false)
            {
                continue;
            }
            let label = match id {
                "premium_interactions" => "Premium requests",
                "chat" => "Chat",
                "completions" => "Completions",
                _ => &id.replace('_', " "),
            };
            if let Some(limit) = number(q.get("entitlement")).filter(|x| *x > 0.0) {
                let remaining = number(q.get("remaining")).unwrap_or(0.0);
                s.meters.push(meter(
                    label,
                    Some(fmt_num((limit - remaining).max(0.0))),
                    Some(fmt_num(limit)),
                    "requests",
                    reset,
                ));
            } else if let Some(remaining) = number(q.get("percent_remaining")) {
                s.meters.push(meter(
                    label,
                    Some(fmt_num((100.0 - remaining).max(0.0))),
                    Some("100".into()),
                    "%",
                    reset,
                ));
            }
        }
    } else if let Some(premium) = v.pointer("/copilot/chat/premium_model_requests") {
        s.meters.push(meter(
            "Premium requests",
            number(premium.get("total_premium_requests_used")).map(fmt_num),
            number(premium.get("total_premium_requests_included")).map(fmt_num),
            "requests",
            parse_time(premium.get("usage_reset_date")),
        ));
    }
}
