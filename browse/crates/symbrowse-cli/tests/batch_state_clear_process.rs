use std::{
    path::Path,
    process::{Command, Output},
    thread,
    time::{Duration, Instant},
};

fn run(binary: &Path, root: &Path, args: &[String]) -> Output {
    let mut command = Command::new(binary);
    command
        .args(args)
        .env_clear()
        .env("HOME", root.join("home"))
        .env("USERPROFILE", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .env("SYMBROWSE_STATE_DIR", root.join("state"))
        .env("SYMBROWSE_DAEMON_LOG", root.join("daemon.log"))
        .env("SYMBROWSE_MODE", "static")
        .env("PATH", root.join("empty-path"));
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    command.output().expect("run isolated symbrowse process")
}

#[test]
fn batch_state_clear_deletes_only_the_isolated_named_state_and_stops_its_daemon() {
    let root = tempfile::tempdir().expect("isolated CLI and daemon roots");
    let binary = Path::new(env!("CARGO_BIN_EXE_symbrowse"));
    let name = "obsolete";
    let session = format!("batch-clear-{}", std::process::id());
    let state_file = root.path().join("state/states/obsolete.json");
    std::fs::create_dir_all(state_file.parent().unwrap()).expect("create isolated state store");
    std::fs::write(&state_file, b"synthetic test-owned state record")
        .expect("create synthetic named-state fixture");

    let clear = format!("state clear {name} --session {session} --json");
    let stop = format!("daemon stop --session {session} --json");
    let output = run(
        binary,
        root.path(),
        &["--json".to_owned(), "batch".to_owned(), clear, stop],
    );

    let mut stopped = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let status = run(
            binary,
            root.path(),
            &[
                "daemon".to_owned(),
                "status".to_owned(),
                "--session".to_owned(),
                session.clone(),
                "--json".to_owned(),
            ],
        );
        if !status.status.success() {
            stopped = true;
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stopped, "isolated daemon did not stop within five seconds");
    assert!(!state_file.exists(), "the named state was not removed");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("batch JSON");
    assert_eq!(report["success"], true);
    let results = report["data"]["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["success"], true);
    assert_eq!(results[0]["data"]["success"], true);
    assert_eq!(results[0]["data"]["data"]["name"], name);
    assert_eq!(results[1]["success"], true);
    assert_eq!(results[1]["data"]["success"], true);
}
