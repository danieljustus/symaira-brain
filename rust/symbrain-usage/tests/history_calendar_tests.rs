#![cfg(unix)]
use symbrain_usage::{Cancellation, scan_grok_history};
#[test]
fn grok_history_dst_calendar() {
    if std::env::var_os("GROK_HISTORY_DST_CHILD").is_none() {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .env_clear()
            .env("TZ", "Europe/Berlin")
            .env("TMPDIR", std::env::temp_dir())
            .env("GROK_HISTORY_DST_CHILD", "1")
            .arg("grok_history_dst_calendar")
            .arg("--exact")
            .arg("--nocapture")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{} {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(String::from_utf8_lossy(&out.stdout).contains("1 passed"));
        return;
    }
    let now = "2026-10-26T00:30:00+01:00"
        .parse::<chrono::DateTime<chrono::FixedOffset>>()
        .unwrap()
        .with_timezone(&chrono::Local);
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join("sessions/p")).unwrap();
    let mut data = String::new();
    for (i, at) in [
        "2026-10-24T21:59:59Z",
        "2026-10-24T22:30:00Z",
        "2026-10-25T22:30:00Z",
        "2026-10-25T23:10:00Z",
    ]
    .iter()
    .enumerate()
    {
        let event = serde_json::json!({"timestamp":at,"params":{"_meta":{"eventId":format!("synthetic-{i}")},"update":{"sessionUpdate":"turn_completed","usage":{"modelUsage":{"grok-build":{"inputTokens":1,"outputTokens":0,"costUsdTicks":0}}}}}});
        data.push_str(&event.to_string());
        data.push('\n');
    }
    std::fs::write(home.path().join("sessions/p/updates.jsonl"), data).unwrap();
    let h = scan_grok_history(home.path(), &now, "local", &Cancellation::default());
    assert_eq!(h.status, "available");
    assert_eq!(h.daily.len(), 3);
    assert_eq!(h.periods.len(), 3);
    for b in h.periods {
        match b.key.as_str() {
            "today" => assert_eq!(b.models[0].events, 1),
            "yesterday" => assert_eq!(b.models[0].events, 2),
            _ => (),
        }
    }
}
