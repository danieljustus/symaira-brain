//! Calendar importer against a faked HTTP client and runner (#761).

use super::*;
use crate::importer::http::fake::FakeHttp;
use crate::importer::runner::fake::Fake;
use crate::importer::test_support::TestDir;
use chrono::TimeZone;

const NOW: fn() -> DateTime<Utc> = || Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).unwrap();
const TOKEN: &str = r#"{"access_token":"tok123","token_type":"Bearer"}"#;
const EVENTS: &str = r#"{"items":[
  {"id":"e1","summary":"Standup","location":"Room 1","start":{"dateTime":"2026-08-02T09:00:00+02:00"},"organizer":{"email":"boss@x.test","name":"Boss"},"attendees":[{"email":"a@x.test","name":"Ann"},{"email":"b@x.test"}],"recurringEventId":"r9"},
  {"id":"e2","summary":"Gone","status":"cancelled","start":{"date":"2026-08-03"}},
  {"id":"e3","summary":"Holiday","start":{"date":"2026-08-04"}},
  {"id":"e4","summary":"No start"}
],"nextPageToken":""}"#;

#[test]
fn discovers_events_with_a_file_token() {
    let dir = TestDir::new("calendar");
    let token = dir.write("token.json", TOKEN);
    let http = FakeHttp::new(vec![("/events?", 200, EVENTS.to_owned())]);
    let vault = Fake::new(vec![]);
    let importer = Calendar::with_io("", &token.to_string_lossy(), false, 0, &http, &vault, &NOW);
    let since = Utc.with_ymd_and_hms(2026, 7, 25, 0, 0, 0).unwrap();
    let sessions = importer.discover_sessions(since).unwrap();
    let ids: Vec<_> = sessions.iter().map(|s| s.session_id.as_str()).collect();
    assert_eq!(ids, ["e1", "e3"]);
    assert_eq!(
        sessions[0].modified_at,
        Utc.with_ymd_and_hms(2026, 8, 2, 7, 0, 0).unwrap()
    );
    assert_eq!(sessions[0].metadata["organizer"], "Boss");
    assert_eq!(sessions[0].metadata["attendees"], "Ann, b@x.test");
    assert_eq!(sessions[0].metadata["recurring"], "true");
    assert_eq!(sessions[0].path, "google-calendar://primary/e1");
    let calls = http.calls.borrow();
    assert_eq!(
        calls[0].0,
        "https://www.googleapis.com/calendar/v3/calendars/primary/events?timeMin=2026-07-25T00:00:00Z&timeMax=2026-08-08T00:00:00Z&singleEvents=true&orderBy=startTime&maxResults=250"
    );
    assert_eq!(
        calls[0].1,
        [("Authorization".to_owned(), "Bearer tok123".to_owned())]
    );
    assert!(vault.calls.borrow().is_empty());
}

#[test]
fn import_event_and_vault_fallback() {
    let event = r#"{"id":"e1","summary":"Review","description":"Agenda text","location":"HQ","start":{"dateTime":"2026-08-02T09:30:00Z"},"end":{"dateTime":"2026-08-02T10:30:00Z"},"organizer":{"email":"boss@x.test"},"attendees":[{"email":"a@x.test"}],"recurringEventId":"r9"}"#;
    let http = FakeHttp::new(vec![("/events/e1", 200, event.to_owned())]);
    let vault = Fake::new(vec![(
        "symvault get google/calendar-token --print",
        Ok(TOKEN),
    )]);
    let importer = Calendar::with_io("work", "", true, 3, &http, &vault, &NOW);
    let session = SessionRef {
        session_id: "e1".into(),
        ..SessionRef::default()
    };
    {
        let fact = &importer.import_session(&session).unwrap()[0];
        assert_eq!(
            fact.content,
            "Calendar event: Review on 2026-08-02 09:30 at HQ\n\nAgenda text"
        );
        assert_eq!(fact.metadata["attendees"], "[\"a@x.test\"]");
        assert_eq!(fact.metadata["end"], "2026-08-02T10:30:00Z");
        assert_eq!(fact.metadata["recurring_id"], "r9");
        assert_eq!(fact.metadata["calendar"], "work");
        assert_eq!(
            fact.timestamp,
            Utc.with_ymd_and_hms(2026, 8, 2, 9, 30, 0).unwrap()
        );
        assert_eq!(
            http.calls.borrow()[0].0,
            "https://www.googleapis.com/calendar/v3/calendars/work/events/e1"
        );
    }
}

#[test]
fn token_and_api_failures() {
    let dir = TestDir::new("calendar-err");
    let http = FakeHttp::new(vec![("/events?", 500, String::new())]);
    let vault = Fake::new(vec![("symvault", Err("exit status 1"))]);
    {
        let none = Calendar::with_io("", "", false, 0, &http, &vault, &NOW);
        assert_eq!(
            none.discover_sessions(NOW()).unwrap_err(),
            "failed to load OAuth token: no token found: set GOOGLE_CALENDAR_TOKEN env or provide --token-path"
        );
    }
    let bad = dir.write("bad.json", "[1]");
    let importer = Calendar::with_io("", &bad.to_string_lossy(), false, 0, &http, &vault, &NOW);
    assert!(
        importer
            .discover_sessions(NOW())
            .unwrap_err()
            .starts_with("failed to load OAuth token: failed to parse token JSON: ")
    );
    let missing = dir.path().join("nope.json").to_string_lossy().into_owned();
    let importer = Calendar::with_io("", &missing, false, 0, &http, &vault, &NOW);
    assert!(
        importer
            .discover_sessions(NOW())
            .unwrap_err()
            .contains("failed to read token file: ")
    );
    let good = dir.write("good.json", TOKEN);
    let importer = Calendar::with_io("", &good.to_string_lossy(), false, 0, &http, &vault, &NOW);
    assert_eq!(
        importer.discover_sessions(NOW()).unwrap_err(),
        "failed to fetch events: API returned status 500"
    );
}
