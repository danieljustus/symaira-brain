#![deny(unsafe_code)]

use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use symbrowse_daemon::{Client, ClientError, ClientOptions, Frame, StartOptions};

fn root() -> PathBuf {
    // Native macOS AF_UNIX endpoints must fit even on hosted runners with a
    // deeply nested TMPDIR. The fixture owns and removes this unique directory.
    let parent = if cfg!(target_os = "macos") {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    parent.join(format!(
        "br-client-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn error(client: &Client) -> symbrowse_daemon::DaemonError {
    match client
        .request(Frame {
            cmd: "daemon.ping".into(),
            ..Default::default()
        })
        .unwrap_err()
    {
        ClientError::Transport(error) => error,
        other => panic!("unexpected client error: {other:?}"),
    }
}

#[test]
fn unavailable_keeps_stable_metadata_and_no_transport_text() {
    let root = root();
    let endpoint = symbrowse_daemon::socket_path(&root, "absent").unwrap();
    let client = Client::new(ClientOptions {
        socket_path: endpoint.clone(),
        session: "absent".into(),
        autostart: false,
        ..Default::default()
    });
    let error = error(&client);
    assert_eq!(error.code, "daemon_unavailable");
    assert_eq!(
        error.message,
        "daemon is unavailable for session \"absent\""
    );
    assert!(error.hint.contains("'symbrowse daemon --session absent'"));
    assert!(error.hint.contains("SYMBROWSE_NO_AUTOSTART"));
    assert_eq!(
        error.details.unwrap(),
        serde_json::json!({"session": "absent", "socket_path": endpoint})
    );
    if root.exists() {
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn launch_failure_has_session_context_and_validates_argv_before_spawning() {
    let root = root();
    for (executable, args, expected) in [
        (
            PathBuf::new(),
            vec!["daemon".into()],
            "daemon executable path is required",
        ),
        (
            std::env::current_exe().unwrap(),
            vec!["not-daemon".into()],
            "daemon start arguments must begin",
        ),
        (
            root.join("missing-executable"),
            vec!["daemon".into()],
            "failed to start daemon",
        ),
    ] {
        let client = Client::new(ClientOptions {
            socket_path: symbrowse_daemon::socket_path(&root, "launch").unwrap(),
            session: "launch".into(),
            autostart: true,
            start: Some(StartOptions {
                executable,
                args,
                log_path: root.join("log/daemon.log"),
            }),
            ..Default::default()
        });
        let error = error(&client);
        assert_eq!(error.code, "daemon_unavailable");
        assert!(
            error
                .message
                .starts_with("failed to start daemon for session \"launch\":")
        );
        assert!(error.message.contains(expected));
        assert!(error.hint.contains("see daemon log at"));
        assert!(!error.message.contains("socket_path"));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn startup_deadline_returns_readiness_failure_and_reaps_its_owned_child() {
    let root = root();
    let log = root.join("private-log/daemon.log");
    let client = Client::new(ClientOptions {
        socket_path: symbrowse_daemon::socket_path(&root, "startup").unwrap(),
        session: "startup".into(),
        autostart: true,
        startup_timeout: Duration::from_millis(500),
        start: Some(StartOptions {
            executable: std::env::current_exe().unwrap(),
            log_path: log.clone(),
            args: vec!["daemon".into(), "--ignored".into(), "--nocapture".into()],
        }),
        ..Default::default()
    });
    let began = Instant::now();
    let error = error(&client);
    assert_eq!(
        error.message,
        "daemon did not become ready for session \"startup\""
    );
    assert!(began.elapsed() >= Duration::from_millis(500));
    assert!(began.elapsed() < Duration::from_secs(3));
    let log_text = fs::read_to_string(&log).unwrap();
    assert!(
        log_text.contains("owned child pid="),
        "child never executed: {log_text}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&log).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(log.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let pid: i32 = log_text
            .lines()
            .find_map(|line| line.strip_prefix("owned child pid="))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
            Err(nix::errno::Errno::ESRCH)
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "owned starter executable explicitly launched by startup deadline test"]
fn daemon_owned_child_fixture() {
    println!("owned child pid={}", std::process::id());
    std::thread::sleep(Duration::from_secs(30));
    panic!("owned child must be terminated at the client startup deadline");
}

#[test]
fn client_uses_resolved_config_and_preserves_explicit_timeout_in_isolated_child() {
    let root = root();
    let config = root.join("config/symbrowse");
    fs::create_dir_all(&config).unwrap();
    let log = root.join("configured-state/daemon.log");
    fs::write(
        config.join("config.toml"),
        format!(
            "state_dir = {:?}\nread_timeout = 7\n",
            root.join("configured-state")
                .to_string_lossy()
                .replace('\\', "/")
        ),
    )
    .unwrap();
    let test = std::env::current_exe().unwrap();
    for (override_seconds, expected) in [(None, "7"), (Some("9"), "9")] {
        let mut command = std::process::Command::new(&test);
        command
            .args(["--exact", "config_owned_child", "--ignored", "--nocapture"])
            .current_dir(&root)
            .env("HOME", &root)
            .env("USERPROFILE", &root)
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env_remove("SYMBROWSE_CONFIG_DIR")
            .env_remove("SYMBROWSE_DAEMON_LOG")
            .env_remove("SYMBROWSE_STATE_DIR")
            .env_remove("SYMBROWSE_READ_TIMEOUT")
            .env("SYMBROWSE_CLIENT_TEST_EXPECTED_SECONDS", expected)
            .env("SYMBROWSE_CLIENT_TEST_EXPECTED_LOG", &log);
        if let Some(value) = override_seconds {
            command.env("SYMBROWSE_READ_TIMEOUT", value);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "isolated environment explicitly executed by config parent"]
fn config_owned_child() {
    let seconds: u64 = std::env::var("SYMBROWSE_CLIENT_TEST_EXPECTED_SECONDS")
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        ClientOptions::default().read_timeout,
        Duration::from_secs(seconds)
    );
    let defaulted = Client::new(ClientOptions {
        read_timeout: Duration::ZERO,
        ..Default::default()
    });
    assert_eq!(
        defaulted.options().read_timeout,
        Duration::from_secs(seconds)
    );
    let explicit = Client::new(ClientOptions {
        read_timeout: Duration::from_millis(333),
        ..Default::default()
    });
    assert_eq!(explicit.options().read_timeout, Duration::from_millis(333));
    assert_eq!(
        symbrowse_daemon::default_log_path(),
        PathBuf::from(std::env::var_os("SYMBROWSE_CLIENT_TEST_EXPECTED_LOG").unwrap())
    );
}

#[test]
#[allow(clippy::result_large_err)] // The existing handler API returns DaemonError inline.
fn explicit_engine_and_policy_checks_stop_incompatible_owner_before_dispatch() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use symbrowse_daemon::{
        PolicyStatus, Server, ServerOptions, SessionRegistry, SessionRegistryOptions,
    };
    for (wrong_engine, without_autostart) in
        [(true, false), (false, false), (true, true), (false, true)]
    {
        let root = root();
        let endpoint = symbrowse_daemon::socket_path(&root, "guarded").unwrap();
        let dispatched = Arc::new(AtomicBool::new(false));
        let marker = dispatched.clone();
        let server = Arc::new(
            Server::new(ServerOptions {
                socket_path: endpoint.clone(),
                session: "guarded".into(),
                engine: "chrome".into(),
                idle_timeout: None,
                registry: Some(Arc::new(SessionRegistry::new(SessionRegistryOptions {
                    user_data_root: root.join("profiles"),
                    ..Default::default()
                }))),
                handler: Some(Arc::new(move |_, _| {
                    marker.store(true, Ordering::Release);
                    Ok((
                        Some(serde_json::json!({"cleared": "must-not-dispatch"})),
                        Vec::new(),
                    ))
                })),
                ..Default::default()
            })
            .unwrap(),
        );
        let running = server.clone();
        let thread = std::thread::spawn(move || running.listen_and_serve());
        let options = ClientOptions {
            socket_path: endpoint,
            session: "guarded".into(),
            autostart: false,
            read_timeout: Duration::from_millis(200),
            ..Default::default()
        };
        let ready = Client::new(options.clone());
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if ready
                .request_without_autostart(Frame {
                    cmd: "daemon.status".into(),
                    ..Default::default()
                })
                .is_ok()
            {
                break;
            }
            if Instant::now() >= deadline {
                server.stop();
                thread.join().unwrap().unwrap();
                panic!("owner did not start");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let checked = Client::new(ClientOptions {
            expected_engine: Some(if wrong_engine { "firefox" } else { "chrome" }.into()),
            expected_policy: if wrong_engine {
                None
            } else {
                Some(PolicyStatus {
                    allow_private: true,
                    ..Default::default()
                })
            },
            ..options
        });
        let frame = Frame {
            cmd: "state.clear".into(),
            session: "guarded".into(),
            args: Some(serde_json::json!({"name": "must-not-dispatch"})),
            ..Default::default()
        };
        let result = if without_autostart {
            checked.request_without_autostart(frame)
        } else {
            checked.request(frame)
        };
        server.stop();
        thread.join().unwrap().unwrap();
        assert!(
            matches!(result, Err(ClientError::Transport(ref error)) if error.code == "daemon_unavailable")
        );
        assert!(
            !dispatched.load(Ordering::Acquire),
            "incompatible owner executed the operation"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
