use super::{Entry, LocalHistory};
use chrono::{DateTime, Duration, TimeZone, Utc};
use std::collections::BTreeMap;
#[derive(Default)]
struct Sum {
    recorded: u64,
    estimated: u64,
    records: usize,
    estimates: usize,
    missing: usize,
}
fn amount(n: u64) -> String {
    let whole = n / 10_000_000_000;
    let rest = n % 10_000_000_000;
    if rest == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{}", format!("{rest:010}").trim_end_matches('0'))
    }
}
fn basis(r: usize, e: usize) -> &'static str {
    match (r > 0, e > 0) {
        (true, true) => "mixed",
        (true, false) => "recorded",
        (false, true) => "estimated",
        _ => "unknown",
    }
}
pub(crate) fn apply_costs<T: TimeZone>(h: &mut LocalHistory, entries: &[Entry], now: &DateTime<T>) {
    let today = now.date_naive();
    let yesterday = today - Duration::days(1);
    let since = today - Duration::days(29);
    let mut sums: BTreeMap<(String, String), Sum> = BTreeMap::new();
    let (mut records, mut estimates) = (0, 0);
    for e in entries {
        let day = e.event.at.with_timezone(&now.timezone()).date_naive();
        if day < since || e.event.at > now.with_timezone(&Utc) {
            continue;
        }
        let mut keys = vec![format!("d:{day}"), "p:last_30_days".into()];
        if day == today {
            keys.push("p:today".into());
        }
        if day == yesterday {
            keys.push("p:yesterday".into());
        }
        if e.event.ticks.is_some() {
            records += 1;
        } else if e.estimate.is_some() {
            estimates += 1;
        }
        for key in keys {
            let s = sums.entry((key, e.event.model.clone())).or_default();
            if let Some(n) = e.event.ticks {
                s.recorded += n;
                s.records += 1;
            } else if let Some(n) = e.estimate {
                s.estimated += n;
                s.estimates += 1;
            } else {
                s.missing += 1;
            }
        }
    }
    h.cost_basis = basis(records, estimates).into();
    h.status = if h.excluded_events > 0 {
        "partial"
    } else if h.daily.is_empty() {
        "no_data"
    } else {
        "available"
    }
    .into();
    for (prefix, buckets) in [("d:", &mut h.daily), ("p:", &mut h.periods)] {
        for b in buckets {
            for m in &mut b.models {
                let s = &sums[&(format!("{prefix}{}", b.key), m.model.clone())];
                m.cost_basis = Some(basis(s.records, s.estimates).into());
                m.recorded_events = Some(s.records);
                m.estimated_events = Some(s.estimates);
                m.missing_cost = s.missing;
                m.recorded_cost = (s.records > 0).then(|| amount(s.recorded));
                m.estimated_cost = (s.estimates > 0).then(|| amount(s.estimated));
                if s.missing > 0 || m.missing_tokens > 0 {
                    h.status = "partial".into();
                }
            }
        }
    }
}
