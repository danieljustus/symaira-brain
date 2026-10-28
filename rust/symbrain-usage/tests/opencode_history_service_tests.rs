#![cfg(unix)]
use std::collections::BTreeMap;
use std::sync::Arc;
use symbrain_usage::{FixtureTransport, ProviderSelection, Response, Service, selected_providers};

#[test]
fn opencode_history_independent_and_private_cleanup() {
    if let Ok(mode) = std::env::var("OPENCODE_HISTORY_TEST_MODE") {
        child(&mode);
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    std::fs::create_dir(&data).unwrap();
    let tmp = root.path().join("tmp");
    std::fs::create_dir(&tmp).unwrap();
    let db = rusqlite::Connection::open(data.join("opencode.db")).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE message(id TEXT PRIMARY KEY,time_created INTEGER,data TEXT)").unwrap();
    let body = r#"{"role":"assistant","providerID":"opencode-go","modelID":"go-model","cost":0,"tokens":{"total":0}}"#;
    db.execute(
        "INSERT INTO message VALUES(?,?,?)",
        rusqlite::params![
            "wal-synthetic",
            chrono::Utc::now().timestamp_millis() - 1000,
            body
        ],
    )
    .unwrap();
    let inventory = || -> BTreeMap<_, _> {
        std::fs::read_dir(&data)
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                (
                    p.file_name().unwrap().to_owned(),
                    std::fs::read(&p).unwrap(),
                )
            })
            .collect()
    };
    for mode in [
        "missing-auth",
        "quota-error",
        "disabled",
        "nonexact",
        "sql-error",
    ] {
        if mode == "sql-error" {
            db.execute_batch("DROP TABLE message; CREATE TABLE message(unrecognized TEXT)")
                .unwrap();
        }
        let before = inventory();
        let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
        cmd.env_clear()
            .env("HOME", root.path())
            .env("TMPDIR", &tmp)
            .env("OPENCODE_DATA_DIR", &data)
            .env("OPENCODE_HISTORY_TEST_MODE", mode)
            .arg("opencode_history_independent_and_private_cleanup")
            .arg("--exact")
            .arg("--nocapture");
        if mode != "disabled" {
            cmd.env(
                "SYMBRAIN_USAGE_OPENCODE_HISTORY",
                if mode == "nonexact" { "true" } else { "1" },
            );
        }
        if mode == "quota-error" {
            cmd.env("OPENCODE_GO_API_KEY", "synthetic");
        }
        let output = cmd.output().unwrap();
        assert!(
            output.status.success(),
            "{mode}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        assert_eq!(before, inventory());
    }
}

fn child(mode: &str) {
    let providers = selected_providers(&ProviderSelection::parse("opencode").unwrap());
    let transport = FixtureTransport::new(BTreeMap::from([(
        "opencode".into(),
        Response {
            status: 401,
            body: b"{}".to_vec(),
            headers: BTreeMap::new(),
        },
    )]));
    let service = Service::with_transport(providers, Arc::new(transport.clone()));
    // History only begins when the report is requested.
    assert_eq!(
        std::fs::read_dir(std::env::var_os("TMPDIR").unwrap())
            .unwrap()
            .count(),
        0
    );
    let report = service.report();
    let p = &report.providers[0];
    match mode {
        "disabled" | "nonexact" => assert!(p.history.is_none()),
        "sql-error" => {
            let h = p.history.as_ref().unwrap();
            assert_eq!(h.status, "unavailable");
            assert!(h.daily.is_empty());
        }
        _ => {
            let h = p.history.as_ref().unwrap();
            assert_eq!(h.status, "available");
            assert_eq!(h.daily[0].models[0].recorded_cost.as_deref(), Some("0"));
            assert_eq!(h.daily[0].models[0].events, 1);
        }
    }
    assert_eq!(
        transport.requests().len(),
        usize::from(mode == "quota-error")
    );
    if mode == "quota-error" {
        assert!(p.error.is_some());
        assert!(p.snapshot.is_none());
    }
    assert_eq!(
        std::fs::read_dir(std::env::var_os("TMPDIR").unwrap())
            .unwrap()
            .count(),
        0
    );
}
