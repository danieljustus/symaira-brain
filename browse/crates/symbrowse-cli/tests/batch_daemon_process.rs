use std::process::Command;

#[test]
fn batch_daemon_stop_keeps_item_failure_and_continues_without_autostart() {
    let root = tempfile::tempdir().expect("isolated home and runtime root");
    let session = format!("batch-stop-{}", std::process::id());
    let stop = format!("daemon stop --session {session} --json");
    let output = Command::new(env!("CARGO_BIN_EXE_symbrowse"))
        .args(["--json", "batch", &stop, "version --json"])
        .env_clear()
        .env("HOME", root.path())
        .env("USERPROFILE", root.path())
        .env("XDG_RUNTIME_DIR", root.path())
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .env("PATH", root.path().join("empty-path"))
        .env("SYMBROWSE_NO_AUTOSTART", "1")
        .output()
        .expect("run isolated batch process");

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("batch JSON");
    assert_eq!(report["success"], true);
    assert_eq!(report["data"]["results"].as_array().unwrap().len(), 2);
    assert_eq!(report["data"]["results"][0]["success"], false);
    assert!(report["data"]["results"][0]["error"].is_string());
    assert_eq!(report["data"]["results"][1]["success"], true);
    assert_eq!(report["data"]["results"][1]["data"]["tool"], "symbrowse");
    assert_eq!(
        std::fs::read_dir(root.path()).unwrap().count(),
        0,
        "a missing daemon must not autostart or create runtime files"
    );
}
