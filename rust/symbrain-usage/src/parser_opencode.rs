//! `OpenCode` JSON-first, JS-literal fallback (`internal/usage/opencode_parse.go`).
use super::{UsageError, UsageSnapshot, meter};
use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

type Window = (f64, Option<f64>);

pub(super) fn parse(body: &[u8], now: DateTime<Utc>) -> Result<UsageSnapshot, UsageError> {
    let mut windows = Vec::new();
    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        collect_windows(&value, &mut windows);
    }
    if windows.is_empty() {
        windows = literal_windows(&String::from_utf8_lossy(body));
    }
    if windows.is_empty() {
        return Err(UsageError::parse("opencode", "missing usage fields"));
    }
    let meters = windows
        .into_iter()
        .take(2)
        .enumerate()
        .map(|(index, (percent, reset))| {
            // Go converts seconds to integer nanoseconds, not integer seconds.
            #[allow(clippy::cast_possible_truncation)]
            let reset = reset.and_then(|value| {
                now.checked_add_signed(chrono::Duration::nanoseconds((value * 1e9) as i64))
            });
            meter(
                if index == 0 { "5h window" } else { "This week" },
                Some(percent.clamp(0.0, 100.0).to_string()),
                Some("100".into()),
                "%",
                reset,
            )
        })
        .collect();
    Ok(UsageSnapshot {
        provider_id: "opencode".into(),
        meters,
        balance: None,
        currency: None,
        fetched_at: now,
        source: "web".into(),
    })
}

fn collect_windows(value: &Value, out: &mut Vec<Window>) {
    // Only the first two windows are observable; don't allocate the rest.
    if out.len() >= 2 {
        return;
    }
    if let Some(map) = value.as_object() {
        let names = [
            "rollingUsage",
            "weeklyUsage",
            "usage",
            "billing",
            "data",
            "result",
        ];
        if names.iter().any(|key| map.contains_key(*key)) {
            for key in names {
                if let Some(child) = map.get(key) {
                    collect_windows(child, out);
                }
            }
            return;
        }
        let percent = [
            "usagePercent",
            "usedPercent",
            "percentUsed",
            "percent",
            "usage_percent",
            "utilization",
            "usage",
        ]
        .iter()
        .find_map(|key| map.get(*key).and_then(Value::as_f64));
        if let Some(percent) = percent {
            let reset = [
                "resetInSec",
                "resetInSeconds",
                "reset_sec",
                "resetsInSec",
                "resetIn",
                "resetSec",
            ]
            .iter()
            .find_map(|key| map.get(*key).and_then(Value::as_f64));
            out.push((percent, reset));
        }
        for child in map.values() {
            collect_windows(child, out);
        }
    } else if let Some(array) = value.as_array() {
        for child in array {
            collect_windows(child, out);
        }
    }
}

fn literal_windows(text: &str) -> Vec<Window> {
    // Go's RE2 \s is ASCII [\t\n\f\r ], not Unicode whitespace or VT.
    static PATTERNS: LazyLock<[Regex; 6]> = LazyLock::new(|| {
        [
            r"rollingUsage[^}]*?usagePercent\s*:\s*([0-9]+(?:\.[0-9]+)?)",
            r"(?:usagePercent|usedPercent|percentUsed|percent)\s*:\s*([0-9]+(?:\.[0-9]+)?)",
            r"rollingUsage[^}]*?resetInSec\s*:\s*([0-9]+)",
            r"(?:resetInSec|resetSeconds|resetIn)\s*:\s*([0-9]+)",
            r"weeklyUsage[^}]*?usagePercent\s*:\s*([0-9]+(?:\.[0-9]+)?)",
            r"weeklyUsage[^}]*?resetInSec\s*:\s*([0-9]+)",
        ]
        .map(|pattern| {
            Regex::new(&pattern.replace(r"\s", r"[\t\n\f\r ]")).expect("constant Go regex")
        })
    });
    let extract = |index: usize| {
        PATTERNS[index]
            .captures(text)
            .map(|capture| capture[1].to_string())
    };
    let number = |text: String| text.parse::<f64>().ok().filter(|value| value.is_finite());
    // Fallback is on an absent match, not on numeric overflow.
    let percent = extract(0).or_else(|| extract(1)).and_then(number);
    let reset = extract(2).or_else(|| extract(3)).and_then(number);
    let (Some(percent), Some(reset)) = (percent, reset) else {
        return Vec::new();
    };
    let mut windows = vec![(percent, Some(reset))];
    if let Some(percent) = extract(4).and_then(number) {
        windows.push((percent, extract(5).and_then(number)));
    }
    windows
}
