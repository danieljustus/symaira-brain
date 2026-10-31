#[cfg(all(test, unix))]
mod opencode_history_tests {
    use super::*;
    #[test]
    fn recorded_admission_bounds() {
        assert!(!ProviderSelection::default().explicit_opencode());
        assert!(
            ProviderSelection::parse("opencode")
                .unwrap()
                .explicit_opencode()
        );
        for mode in ["channels", "rows", "row-bytes", "symlink"] {
            let root = tempfile::tempdir().unwrap();
            match mode {
                "channels" => {
                    for i in 0..9 {
                        fs::write(root.path().join(format!("opencode-{i}.db")), []).unwrap();
                    }
                }
                "symlink" => std::os::unix::fs::symlink(
                    root.path().join("absent"),
                    root.path().join("opencode.db"),
                )
                .unwrap(),
                _ => {
                    let db = rusqlite::Connection::open(root.path().join("opencode.db")).unwrap();
                    db.execute_batch(
                        "CREATE TABLE message(id TEXT PRIMARY KEY,time_created INTEGER,data TEXT)",
                    )
                    .unwrap();
                    db.execute_batch(if mode == "rows" { "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<10001) INSERT INTO message SELECT CAST(x AS TEXT),1,'{}' FROM n" } else { "INSERT INTO message VALUES('oversized',1,replace(hex(zeroblob(131073)),'0','x'))" }).unwrap();
                }
            }
            let h = scan_opencode_history(
                root.path(),
                &chrono::Utc::now(),
                "UTC",
                &crate::Cancellation::default(),
            );
            assert_eq!(h.status, "unavailable", "{mode}");
            assert!(h.daily.is_empty());
        }
    }
    #[test]
    fn recorded_shared_history() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/opencode-history-cases.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let root = tempfile::tempdir().unwrap();
            for row in case["rows"].as_array().unwrap() {
                let db = rusqlite::Connection::open(root.path().join(row["db"].as_str().unwrap()))
                    .unwrap();
                db.execute_batch("CREATE TABLE IF NOT EXISTS message(id TEXT PRIMARY KEY,time_created INTEGER,data TEXT); CREATE TABLE IF NOT EXISTS session_message(id TEXT PRIMARY KEY,time_created INTEGER,type TEXT,data TEXT)").unwrap();
                if row["table"] == "message" {
                    db.execute(
                        "INSERT INTO message VALUES(?,?,?)",
                        rusqlite::params![
                            row["id"].as_str().unwrap(),
                            row["ms"].as_i64().unwrap(),
                            row["data"].as_str().unwrap()
                        ],
                    )
                    .unwrap();
                } else {
                    db.execute(
                        "INSERT INTO session_message VALUES(?,?,?,?)",
                        rusqlite::params![
                            row["id"].as_str().unwrap(),
                            row["ms"].as_i64().unwrap(),
                            row["kind"].as_str().unwrap(),
                            row["data"].as_str().unwrap()
                        ],
                    )
                    .unwrap();
                }
            }
            let inventory = || -> std::collections::BTreeMap<_, _> {
                fs::read_dir(root.path())
                    .unwrap()
                    .map(|e| {
                        let p = e.unwrap().path();
                        (p.file_name().unwrap().to_owned(), fs::read(&p).unwrap())
                    })
                    .collect()
            };
            let before = inventory();
            let now = case["now"]
                .as_str()
                .unwrap()
                .parse::<chrono::DateTime<chrono::Utc>>()
                .unwrap();
            let actual =
                scan_opencode_history(root.path(), &now, "UTC", &crate::Cancellation::default());
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                case["expected"],
                "{}",
                case["name"]
            );
            assert_eq!(before, inventory());
            let cancel = crate::Cancellation::default();
            cancel.cancel();
            assert_eq!(
                scan_opencode_history(root.path(), &now, "UTC", &cancel).status,
                "unavailable"
            );
        }
    }
}
