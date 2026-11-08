use super::*;
use chrono::TimeZone;

fn now() -> DateTime<Utc> {
    // 2026-07-30 12:00 UTC, a Thursday (same reference as the Go tests).
    Utc.with_ymd_and_hms(2026, 7, 30, 12, 0, 0).unwrap()
}

fn day(w: Option<DateTime<Utc>>) -> String {
    w.map(|t| t.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

type Case = (&'static str, &'static str, &'static str, &'static str);

#[test]
fn extract_table() {
    let cases: &[Case] = &[
        ("yesterday", "2026-07-29", "2026-07-30", "yesterday"),
        ("today", "2026-07-30", "2026-07-31", "today"),
        ("last week", "2026-07-20", "2026-07-27", "last week"),
        ("this week", "2026-07-27", "2026-08-03", "this week"),
        ("last month", "2026-06-01", "2026-07-01", "last month"),
        ("this month", "2026-07-01", "2026-08-01", "this month"),
        ("last year", "2025-01-01", "2026-01-01", "last year"),
        ("this year", "2026-01-01", "2027-01-01", "this year"),
        ("in the last 3 days", "2026-07-27", "", "in the last 3 days"),
        (
            "in the past 2 weeks",
            "2026-07-16",
            "",
            "in the past 2 weeks",
        ),
        (
            "in the last 1 month",
            "2026-06-30",
            "",
            "in the last 1 month",
        ),
        (
            "in the last 2 years",
            "2024-07-30",
            "",
            "in the last 2 years",
        ),
        ("5 days ago", "2026-07-25", "", "5 days ago"),
        ("3 weeks ago", "2026-07-09", "", "3 weeks ago"),
        ("gestern", "2026-07-29", "2026-07-30", "gestern"),
        ("heute", "2026-07-30", "2026-07-31", "heute"),
        ("letzte Woche", "2026-07-20", "2026-07-27", "letzte Woche"),
        ("letzten Monat", "2026-06-01", "2026-07-01", "letzten Monat"),
        ("letztes Jahr", "2025-01-01", "2026-01-01", "letztes Jahr"),
        ("diese Woche", "2026-07-27", "2026-08-03", "diese Woche"),
        ("diesen Monat", "2026-07-01", "2026-08-01", "diesen Monat"),
        ("dieses Jahr", "2026-01-01", "2027-01-01", "dieses Jahr"),
        (
            "in den letzten 3 Tagen",
            "2026-07-27",
            "",
            "in den letzten 3 Tagen",
        ),
        ("vor 5 Tagen", "2026-07-25", "", "vor 5 Tagen"),
        ("vor 2 Wochen", "2026-07-16", "", "vor 2 Wochen"),
        ("vor 3 Monaten", "2026-05-01", "", "vor 3 Monaten"),
        ("vor 1 Jahr", "2025-07-30", "", "vor 1 Jahr"),
        (
            "database config yesterday migration",
            "2026-07-29",
            "2026-07-30",
            "yesterday",
        ),
        (
            "Server Konfiguration letzte Woche geändert",
            "2026-07-20",
            "2026-07-27",
            "letzte Woche",
        ),
        ("YESTERDAY", "2026-07-29", "2026-07-30", "YESTERDAY"),
        ("Last Week", "2026-07-20", "2026-07-27", "Last Week"),
    ];
    for (query, from, to, matched) in cases {
        let got = extract(query, now()).unwrap_or_else(|| panic!("{query:?} gave None"));
        assert_eq!(got.confidence, "high", "{query}");
        assert_eq!(got.matched_text, *matched, "{query}");
        assert_eq!(day(got.window.from), *from, "{query} from");
        assert_eq!(day(got.window.to), *to, "{query} to");
    }
}

#[test]
fn extract_none_table() {
    for query in [
        "",
        "  ",
        "database configuration settings",
        "soon",
        "next week",
        "tomorrow",
        "bald",
        "nächste Woche",
        "letztes mal",
        "morgen",
        "a",
        "0 days ago",
    ] {
        assert_eq!(extract(query, now()), None, "{query:?}");
    }
}

#[test]
fn week_boundaries_sunday_and_january() {
    let sunday = Utc.with_ymd_and_hms(2026, 8, 2, 8, 0, 0).unwrap();
    let w = extract("this week", sunday).unwrap().window;
    assert_eq!(
        (day(w.from), day(w.to)),
        ("2026-07-27".into(), "2026-08-03".into())
    );
    let jan = Utc.with_ymd_and_hms(2026, 1, 15, 8, 0, 0).unwrap();
    let w = extract("last month", jan).unwrap().window;
    assert_eq!(
        (day(w.from), day(w.to)),
        ("2025-12-01".into(), "2026-01-01".into())
    );
}

#[test]
fn matched_text_and_precedence() {
    // The named period outranks the numeric form, as in Go.
    let got = extract("today and 5 days ago", now()).unwrap();
    assert_eq!(got.matched_text, "today");
}
