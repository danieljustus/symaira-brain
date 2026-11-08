//! Natural-language time-window extraction (ported for #760 from
//! `internal/memory/temporal/temporal.go` at v0.12.0).
//!
//! Recognises English and German expressions ("yesterday", "letzte Woche",
//! "in the last 3 days", "vor 5 Tagen", ...) in a search query and returns
//! the matching UTC window. The clock is injected; weak or future signals
//! ("tomorrow", "bald") yield `None`.

use chrono::{DateTime, Datelike, Duration, Months, NaiveDate, Utc};
use regex::Regex;
use std::sync::OnceLock;

/// Optional lower/upper bound of a time window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TimeWindow {
    /// Inclusive start.
    pub from: Option<DateTime<Utc>>,
    /// Exclusive end.
    pub to: Option<DateTime<Utc>>,
}

/// An extracted constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    /// The window.
    pub window: TimeWindow,
    /// Always `"high"` (kept for wire parity with Go).
    pub confidence: &'static str,
    /// The text fragment that triggered the extraction.
    pub matched_text: String,
}

#[derive(Clone, Copy)]
enum Rule {
    Yesterday,
    Today,
    LastWeek,
    ThisWeek,
    LastMonth,
    ThisMonth,
    LastYear,
    ThisYear,
    /// Hours per unit for "last/past/ago N unit".
    Back(i64),
}

const DAY: i64 = 24;
const WEEK: i64 = 7 * 24;
const MONTH: i64 = 30 * 24;
const YEAR: i64 = 365 * 24;

/// Go's `\b`, `\s` and `\d` are ASCII-only; mirror that explicitly.
fn compile(pattern: &str) -> Regex {
    let converted = pattern
        .replace("\\b", "(?-u:\\b)")
        .replace("\\s", "[ \\t\\n\\x0c\\r]")
        .replace("\\d", "[0-9]");
    Regex::new(&format!("(?i){converted}")).expect("static temporal pattern")
}

fn rules() -> &'static Vec<(Regex, Rule)> {
    static RULES: OnceLock<Vec<(Regex, Rule)>> = OnceLock::new();
    RULES.get_or_init(|| {
        // Order is the Go evaluation order; the first matching rule wins.
        let table: &[(&str, Rule)] = &[
            (r"\byesterday\b", Rule::Yesterday),
            (r"\bgestern\b", Rule::Yesterday),
            (r"\btoday\b", Rule::Today),
            (r"\bheute\b", Rule::Today),
            (r"\blast\s+week\b", Rule::LastWeek),
            (r"\bletzt(?:e|en)\s+woche\b", Rule::LastWeek),
            (r"\bthis\s+week\b", Rule::ThisWeek),
            (r"\bdiese\s+woche\b", Rule::ThisWeek),
            (r"\blast\s+month\b", Rule::LastMonth),
            (r"\bletzt(?:e|en|er)\s+monat\b", Rule::LastMonth),
            (r"\bthis\s+month\b", Rule::ThisMonth),
            (r"\bdiese(?:n|s|)\s+monat\b", Rule::ThisMonth),
            (r"\blast\s+year\b", Rule::LastYear),
            (r"\bletzt(?:e|es)\s+jahr\b", Rule::LastYear),
            (r"\bthis\s+year\b", Rule::ThisYear),
            (r"\bdiese(?:s|)\s+jahr\b", Rule::ThisYear),
            (
                r"(?:in\s+)?(?:the\s+)?(?:last|past)\s+(\d+)\s+days?\b",
                Rule::Back(DAY),
            ),
            (
                r"(?:in\s+)?den\s+letzten\s+(\d+)\s+tagen?\b",
                Rule::Back(DAY),
            ),
            (
                r"(?:in\s+)?(?:the\s+)?(?:last|past)\s+(\d+)\s+weeks?\b",
                Rule::Back(WEEK),
            ),
            (
                r"(?:in\s+)?den\s+letzten\s+(\d+)\s+wochen?\b",
                Rule::Back(WEEK),
            ),
            (
                r"(?:in\s+)?(?:the\s+)?(?:last|past)\s+(\d+)\s+months?\b",
                Rule::Back(MONTH),
            ),
            (
                r"(?:in\s+)?den\s+letzten\s+(\d+)\s+monaten?\b",
                Rule::Back(MONTH),
            ),
            (
                r"(?:in\s+)?(?:the\s+)?(?:last|past)\s+(\d+)\s+years?\b",
                Rule::Back(YEAR),
            ),
            (
                r"(?:in\s+)?den\s+letzten\s+(\d+)\s+jahren?\b",
                Rule::Back(YEAR),
            ),
            (r"(\d+)\s+days?\s+ago\b", Rule::Back(DAY)),
            (r"\bvor\s+(\d+)\s+tage?n?\b", Rule::Back(DAY)),
            (r"(\d+)\s+weeks?\s+ago\b", Rule::Back(WEEK)),
            (r"\bvor\s+(\d+)\s+wochen?\b", Rule::Back(WEEK)),
            (r"(\d+)\s+months?\s+ago\b", Rule::Back(MONTH)),
            (r"\bvor\s+(\d+)\s+monate?n?\b", Rule::Back(MONTH)),
            (r"(\d+)\s+years?\s+ago\b", Rule::Back(YEAR)),
            (r"\bvor\s+(\d+)\s+jahre?n?\b", Rule::Back(YEAR)),
        ];
        table.iter().map(|(p, r)| (compile(p), *r)).collect()
    })
}

fn midnight(date: NaiveDate) -> DateTime<Utc> {
    date.and_hms_opt(0, 0, 0)
        .expect("midnight is valid")
        .and_utc()
}

fn day_window(start: NaiveDate) -> TimeWindow {
    let from = midnight(start);
    TimeWindow {
        from: Some(from),
        to: Some(from + Duration::hours(DAY)),
    }
}

fn monday_of(date: NaiveDate) -> NaiveDate {
    date - Duration::days(i64::from(date.weekday().num_days_from_monday()))
}

fn span(start: NaiveDate, months: u32) -> Option<TimeWindow> {
    let end = start.checked_add_months(Months::new(months))?;
    Some(TimeWindow {
        from: Some(midnight(start)),
        to: Some(midnight(end)),
    })
}

fn window(rule: Rule, now: DateTime<Utc>, count: Option<&str>) -> Option<TimeWindow> {
    let today = now.date_naive();
    let first_of_month = today.with_day(1)?;
    match rule {
        Rule::Yesterday => Some(day_window(today.pred_opt()?)),
        Rule::Today => Some(day_window(today)),
        Rule::LastWeek | Rule::ThisWeek => {
            let this = monday_of(today);
            let start = if matches!(rule, Rule::LastWeek) {
                this - Duration::days(7)
            } else {
                this
            };
            let from = midnight(start);
            Some(TimeWindow {
                from: Some(from),
                to: Some(from + Duration::hours(WEEK)),
            })
        }
        Rule::LastMonth => span(first_of_month.checked_sub_months(Months::new(1))?, 1),
        Rule::ThisMonth => span(first_of_month, 1),
        Rule::LastYear => span(NaiveDate::from_ymd_opt(today.year() - 1, 1, 1)?, 12),
        Rule::ThisYear => span(NaiveDate::from_ymd_opt(today.year(), 1, 1)?, 12),
        Rule::Back(hours) => {
            let n: i64 = count?.parse().ok().filter(|n| *n > 0)?;
            let delta = Duration::try_hours(n.checked_mul(hours)?)?;
            Some(TimeWindow {
                from: Some(now.checked_sub_signed(delta)?),
                to: None,
            })
        }
    }
}

/// Scans `s` for a supported expression relative to `now`.
#[must_use]
pub fn extract(s: &str, now: DateTime<Utc>) -> Option<Extracted> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    for (re, rule) in rules() {
        if let Some(caps) = re.captures(s) {
            let whole = caps.get(0)?.as_str();
            // Go returns nil (not the next rule) when the first match yields
            // a non-positive count.
            return window(*rule, now, caps.get(1).map(|m| m.as_str())).map(|window| Extracted {
                window,
                confidence: "high",
                matched_text: whole.to_owned(),
            });
        }
    }
    None
}

#[cfg(test)]
#[path = "temporal_tests.rs"]
mod tests;
