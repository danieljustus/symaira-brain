#![deny(unsafe_code)]

#[cfg(unix)]
#[allow(clippy::result_large_err)]
mod unix {
    use std::{
        fs,
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
            mpsc,
        },
        thread,
        time::{Duration, Instant},
    };

    use symbrowse_daemon::{
        Client, ClientError, ClientOptions, Frame, Server, ServerOptions, SessionSpec,
        StartOptions, codes,
    };

    static NEXT: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn dmn006_active_handler_survives_idle_deadline() {
        let root = tempfile::tempdir().expect("temporary daemon root");
        let socket = root.path().join("active.sock");
        let mut spec = SessionSpec::for_session("active");
        spec.socket_path = socket.clone();
        spec.state_dir = root.path().join("state");
        spec.cache_dir = root.path().join("cache");
        spec.idle_timeout = Some(Duration::from_millis(100));
        spec.operation_timeout = Duration::from_millis(700);
        let server = Arc::new(
            Server::new(ServerOptions {
                session_spec: Some(spec),
                handler: Some(Arc::new(|_, _| {
                    thread::sleep(Duration::from_millis(300));
                    Ok((Some(serde_json::json!({"done": true})), Vec::new()))
                })),
                ..Default::default()
            })
            .expect("create daemon"),
        );
        let running = server.clone();
        let thread = thread::spawn(move || running.listen_and_serve());
        wait_for_socket(&socket);
        let client = Client::new(ClientOptions {
            socket_path: socket,
            session: "active".into(),
            autostart: false,
            ..Default::default()
        });
        let response = client
            .request(Frame {
                cmd: "slow".into(),
                ..Default::default()
            })
            .expect("active handler keeps daemon available");
        assert!(response.success, "active request response = {response:?}");
        server.stop();
        assert!(thread.join().expect("join daemon").is_ok());
    }

    #[test]
    fn autostart_retries_until_the_daemon_is_ready() {
        let root = tempfile::tempdir().expect("temporary autostart root");
        let socket = root.path().join("retry.sock");
        let launched = root.path().join("launched");
        let completed = root.path().join("completed");
        let log_path = root.path().join("daemon.log");
        let test_binary = std::env::current_exe().expect("current test executable");
        let command = format!(
            "touch {}; sleep 0.12; {} --ignored --exact unix::autostart_daemon_child --nocapture; result=$?; touch {}; exit $result",
            shell_quote(&launched),
            shell_quote(&test_binary),
            shell_quote(&completed),
        );
        let client = Client::new(ClientOptions {
            socket_path: socket.clone(),
            session: "retry-session".into(),
            startup_timeout: Duration::from_secs(3),
            autostart: true,
            start: Some(StartOptions {
                executable: PathBuf::from("/bin/sh"),
                log_path,
                args: vec![
                    "-c".into(),
                    format!(
                        "export SYMBROWSE_DMN008_SOCKET={}; export SYMBROWSE_DMN008_SESSION=retry-session; {}",
                        shell_quote(&socket),
                        command
                    ),
                ],
            }),
            ..Default::default()
        });

        let response = client
            .request(Frame {
                cmd: "daemon.status".into(),
                ..Default::default()
            })
            .expect("client retries while autostarted daemon becomes ready");
        assert!(response.success, "status response = {response:?}");
        assert!(launched.exists(), "autostart wrapper ran");
        assert!(socket.exists(), "child daemon published its socket");

        let stopped = client
            .request(Frame {
                cmd: "daemon.stop".into(),
                ..Default::default()
            })
            .expect("stop autostarted daemon");
        assert!(stopped.success, "stop response = {stopped:?}");
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && !completed.exists() {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(completed.exists(), "autostart child exited after stop");
    }

    #[test]
    #[ignore = "launched as a child fixture by autostart_retries_until_the_daemon_is_ready"]
    fn autostart_daemon_child() {
        let socket_path = PathBuf::from(
            std::env::var_os("SYMBROWSE_DMN008_SOCKET").expect("fixture socket path"),
        );
        let session = std::env::var("SYMBROWSE_DMN008_SESSION").expect("fixture session");
        let server = Server::new(ServerOptions {
            socket_path,
            session,
            idle_timeout: None,
            ..Default::default()
        })
        .expect("child fixture server");
        server.listen_and_serve().expect("child fixture serve");
    }

    #[test]
    fn concurrent_clients_autostart_one_shared_daemon() {
        const CLIENTS: usize = 4;
        let root = root("concurrent-client-autostart");
        fs::create_dir_all(&root).expect("create concurrent autostart root");
        let socket = root.join("race.sock");
        let launch_gate = root.join("launch-gate");
        let test_binary = std::env::current_exe().expect("current test executable");
        let barrier = Arc::new(std::sync::Barrier::new(CLIENTS));
        let mut clients = Vec::with_capacity(CLIENTS);

        for index in 0..CLIENTS {
            let barrier = barrier.clone();
            let socket_for_client = socket.clone();
            let root_for_client = root.clone();
            let gate_for_client = launch_gate.clone();
            let binary_for_client = test_binary.clone();
            clients.push(thread::spawn(move || {
                let launched = root_for_client.join(format!("launched-{index}"));
                let completed = root_for_client.join(format!("completed-{index}"));
                let command = format!(
                    "touch {}; while [ ! -f {} ]; do sleep 0.01; done; export SYMBROWSE_DMN008_SOCKET={}; export SYMBROWSE_DMN008_SESSION=client-race; {} --ignored --exact unix::autostart_daemon_child --nocapture; result=$?; touch {}; exit $result",
                    shell_quote(&launched),
                    shell_quote(&gate_for_client),
                    shell_quote(&socket_for_client),
                    shell_quote(&binary_for_client),
                    shell_quote(&completed),
                );
                let client = Client::new(ClientOptions {
                    socket_path: socket_for_client,
                    session: "client-race".into(),
                    startup_timeout: Duration::from_secs(8),
                    autostart: true,
                    start: Some(StartOptions {
                        executable: PathBuf::from("/bin/sh"),
                        log_path: root_for_client.join(format!("daemon-{index}.log")),
                        args: vec!["-c".into(), command],
                    }),
                    ..Default::default()
                });

                barrier.wait();
                let response = client.request(Frame {
                    cmd: "daemon.status".into(),
                    ..Default::default()
                });
                (response, launched, completed)
            }));
        }

        let marker_count = |prefix: &str| {
            (0..CLIENTS)
                .filter(|index| root.join(format!("{prefix}-{index}")).exists())
                .count()
        };
        let launch_deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < launch_deadline && marker_count("launched") < CLIENTS {
            thread::sleep(Duration::from_millis(10));
        }
        fs::write(&launch_gate, b"go").expect("release competing daemon launchers");
        assert_eq!(
            marker_count("launched"),
            CLIENTS,
            "all clients must autostart"
        );

        let results: Vec<_> = clients
            .into_iter()
            .map(|client| client.join().expect("join concurrent client"))
            .collect();
        let launched_count = results
            .iter()
            .filter(|(_, launched, _)| launched.exists())
            .count();

        let losing_deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < losing_deadline && marker_count("completed") < CLIENTS - 1 {
            thread::sleep(Duration::from_millis(10));
        }
        let losers_completed = marker_count("completed");

        if socket.exists() {
            let stopper = Client::new(ClientOptions {
                socket_path: socket.clone(),
                session: "client-race".into(),
                autostart: false,
                ..Default::default()
            });
            let stopped = stopper
                .request_without_autostart(Frame {
                    cmd: "daemon.stop".into(),
                    ..Default::default()
                })
                .expect("stop the shared autostarted daemon");
            assert!(stopped.success, "stop response = {stopped:?}");
        }

        for (response, _, _) in &results {
            let response = response.as_ref().expect("concurrent client request");
            assert!(response.success, "status response = {response:?}");
        }
        assert!(
            launched_count == CLIENTS,
            "expected all competing autostart attempts, observed {launched_count}"
        );
        assert_eq!(
            losers_completed,
            CLIENTS - 1,
            "losing launchers must exit before stop"
        );

        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline
            && results.iter().any(|(_, _, completed)| !completed.exists())
        {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            results.iter().all(|(_, _, completed)| completed.exists()),
            "all competing daemon launcher processes must exit after stop"
        );
        fs::remove_dir_all(root).expect("remove concurrent autostart root");
    }

    #[test]
    fn dmn006_client_read_timeout_has_operation_timeout_code_and_diagnostics() {
        let root = root("client-timeout");
        let socket = root.join("default.sock");
        let marker = root.join("autostarted");
        let log = root.join("daemon.log");
        let server = Arc::new(
            Server::new(ServerOptions {
                socket_path: socket.clone(),
                session: "default".into(),
                idle_timeout: None,
                operation_timeout: Duration::from_millis(500),
                handler: Some(Arc::new(|frame, _| {
                    if frame.cmd == "slow" {
                        thread::sleep(Duration::from_millis(100));
                    }
                    Ok((Some(serde_json::json!({"pong": true})), Vec::new()))
                })),
                ..Default::default()
            })
            .unwrap(),
        );
        let running = server.clone();
        let thread = thread::spawn(move || running.listen_and_serve());
        wait_for_socket(&socket);

        let client = Client::new(ClientOptions {
            socket_path: socket.clone(),
            session: "slow-sess".into(),
            read_timeout: Duration::from_millis(30),
            startup_timeout: Duration::from_millis(30),
            autostart: true,
            start: Some(StartOptions {
                executable: PathBuf::from("/bin/sh"),
                log_path: log,
                args: vec!["-c".into(), format!("touch {}; sleep 1", marker.display())],
            }),
            ..Default::default()
        });

        let error = client
            .request(Frame {
                cmd: "slow".into(),
                ..Default::default()
            })
            .expect_err("a client deadline must fail the request");
        match error {
            ClientError::Transport(error) => {
                assert_eq!(error.code, codes::OPERATION_TIMEOUT);
                assert_eq!(error.message, "daemon response timed out after 30ms");
                assert_eq!(
                    error.hint,
                    "increase timeout with SYMBROWSE_READ_TIMEOUT or inspect daemon logs for session \"slow-sess\""
                );
                assert!(
                    !error
                        .message
                        .contains(&socket.to_string_lossy().to_string()),
                    "timeout message leaked socket path: {}",
                    error.message
                );
                let details = error.details.expect("timeout diagnostics");
                assert_eq!(details["session"], "slow-sess");
                assert_eq!(details["socket_path"], socket.to_string_lossy().as_ref());
                let seconds = details["timeout_seconds"]
                    .as_f64()
                    .expect("timeout seconds");
                assert!((seconds - 0.03).abs() < f64::EPSILON);
            }
            other => panic!("client timeout = {other:?}"),
        }
        thread::sleep(Duration::from_millis(50));
        assert!(
            !marker.exists(),
            "a timed-out live daemon request must not start a second daemon"
        );

        server.stop();
        assert!(thread.join().unwrap().is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn autostart_failures_report_stable_session_context() {
        let root = root("autostart-failure");
        fs::create_dir_all(&root).unwrap();
        let socket = root.join("default.sock");
        let client = Client::new(ClientOptions {
            socket_path: socket.clone(),
            session: "auto-fail".into(),
            autostart: true,
            start: Some(StartOptions {
                executable: root.join("missing-daemon"),
                log_path: root.join("daemon.log"),
                args: vec!["daemon".into()],
            }),
            ..Default::default()
        });

        let error = client
            .request(Frame {
                cmd: "ping".into(),
                ..Default::default()
            })
            .expect_err("missing daemon executable must fail");
        let ClientError::Transport(error) = error else {
            panic!("autostart failure = {error:?}");
        };
        assert_eq!(error.code, codes::DAEMON_UNAVAILABLE);
        assert!(
            error
                .message
                .contains("failed to start daemon for session \"auto-fail\"")
        );
        assert!(error.hint.contains("symbrowse daemon --session auto-fail"));
        let details = error.details.expect("session diagnostics");
        assert_eq!(details["session"], "auto-fail");
        assert_eq!(details["socket_path"], socket.to_string_lossy().as_ref());
        assert!(
            !error
                .message
                .contains(&socket.to_string_lossy().to_string())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_daemon_with_autostart_disabled_does_not_spawn_child() {
        let root = tempfile::tempdir().expect("temporary no-autostart root");
        let socket = root.path().join("missing.sock");
        let client = Client::new(ClientOptions {
            socket_path: socket.clone(),
            session: "no-auto".into(),
            autostart: false,
            start: Some(StartOptions {
                // If the client accidentally attempts to autostart, this
                // deliberately missing executable changes the returned error.
                executable: root.path().join("must-not-run"),
                log_path: root.path().join("daemon.log"),
                args: vec!["daemon".into()],
            }),
            ..Default::default()
        });

        let error = client
            .request(Frame {
                cmd: "ping".into(),
                ..Default::default()
            })
            .expect_err("missing daemon must fail while autostart is disabled");
        let ClientError::Transport(error) = error else {
            panic!("missing-daemon error = {error:?}");
        };
        assert_eq!(error.code, codes::DAEMON_UNAVAILABLE);
        assert_eq!(
            error.message,
            "daemon is unavailable for session \"no-auto\""
        );
        assert!(
            error
                .hint
                .contains("autostart disabled via SYMBROWSE_NO_AUTOSTART")
        );
        let details = error.details.expect("missing-daemon diagnostics");
        assert_eq!(details["session"], "no-auto");
        assert_eq!(details["socket_path"], socket.to_string_lossy().as_ref());
        assert!(
            !root.path().join("daemon.log").exists(),
            "disabled autostart must not create a daemon log"
        );
    }

    #[test]
    fn autostart_timeout_reports_not_ready_and_stops_child() {
        let root = root("autostart-timeout");
        fs::create_dir_all(&root).unwrap();
        let socket = root.join("default.sock");
        let client = Client::new(ClientOptions {
            socket_path: socket.clone(),
            session: "timeout-sess".into(),
            startup_timeout: Duration::from_millis(50),
            autostart: true,
            start: Some(StartOptions {
                executable: PathBuf::from("/bin/sleep"),
                log_path: root.join("daemon.log"),
                args: vec!["2".into()],
            }),
            ..Default::default()
        });

        let started = Instant::now();
        let error = client
            .request(Frame {
                cmd: "ping".into(),
                ..Default::default()
            })
            .expect_err("a child that never publishes the socket must time out");
        assert!(started.elapsed() < Duration::from_secs(1));
        let ClientError::Transport(error) = error else {
            panic!("autostart timeout = {error:?}");
        };
        assert_eq!(error.code, codes::DAEMON_UNAVAILABLE);
        assert!(error.message.contains("daemon did not become ready"));
        assert!(error.message.contains("timeout-sess"));
        assert!(
            error
                .hint
                .contains("symbrowse daemon --session timeout-sess")
        );
        let details = error.details.expect("session diagnostics");
        assert_eq!(details["session"], "timeout-sess");
        assert_eq!(details["socket_path"], socket.to_string_lossy().as_ref());
        assert!(
            !error
                .message
                .contains(&socket.to_string_lossy().to_string())
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn status_failure_is_preserved_without_stopping_daemon() {
        let root = root("status-failure");
        fs::create_dir_all(&root).unwrap();
        let socket = root.join("default.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let (commands, seen) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let thread = thread::spawn(move || {
            let mut status_seen = false;
            loop {
                let (stream, _) = listener.accept().expect("accept daemon fixture");
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    continue;
                }
                let frame: Frame = serde_json::from_str(&line).unwrap();
                let _ = commands.send(frame.cmd.clone());
                let response = if frame.cmd == "daemon.status" {
                    status_seen = true;
                    symbrowse_daemon::error_response(
                        codes::OPERATION_FAILED,
                        "status fixture failed",
                    )
                } else {
                    symbrowse_daemon::success_response(
                        Some(serde_json::json!({"ok": true})),
                        Vec::new(),
                    )
                };
                let mut writer = reader.into_inner();
                writer
                    .write_all(
                        format!("{}\n", serde_json::to_string(&response).unwrap()).as_bytes(),
                    )
                    .expect("write daemon fixture response");
                writer.flush().expect("flush daemon fixture response");
                if status_seen {
                    released
                        .recv_timeout(Duration::from_secs(2))
                        .expect("wait for cancellation fixture release");
                    break;
                }
            }
        });

        let client = Client::new(ClientOptions {
            socket_path: socket,
            session: "default".into(),
            read_timeout: Duration::from_millis(100),
            autostart: false,
            ..Default::default()
        });
        let error = client
            .request(Frame {
                cmd: "daemon.ping".into(),
                ..Default::default()
            })
            .expect_err("failed status must be returned to the caller");
        match error {
            ClientError::Transport(error) => {
                assert_eq!(error.code, codes::OPERATION_FAILED);
                assert_eq!(error.message, "status fixture failed");
            }
            other => panic!("status failure = {other:?}"),
        }
        release.send(()).unwrap();
        thread.join().unwrap();
        let seen: Vec<_> = seen.try_iter().collect();
        assert_eq!(seen, ["daemon.ping", "daemon.status"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn engine_and_policy_mismatches_warn_without_stopping_daemon() {
        let root = root("config-mismatch");
        fs::create_dir_all(&root).unwrap();
        let socket = root.join("default.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let (commands, seen) = mpsc::channel();
        let thread = thread::spawn(move || {
            for _ in 0..2 {
                let (stream, _) = listener.accept().expect("accept daemon fixture");
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).expect("read daemon request");
                let frame: Frame = serde_json::from_str(&line).expect("decode daemon request");
                commands.send(frame.cmd.clone()).unwrap();
                let response = if frame.cmd == "daemon.status" {
                    symbrowse_daemon::success_response(
                        Some(serde_json::json!({
                            "session": "default",
                            "engine": "chrome",
                            "policy": {
                                "allowed_domains": [],
                                "ssrf_enabled": false,
                                "fetch_ssrf_enabled": false,
                                "allow_private": false
                            }
                        })),
                        Vec::new(),
                    )
                } else {
                    symbrowse_daemon::success_response(
                        Some(serde_json::json!({"ok": true})),
                        Vec::new(),
                    )
                };
                let mut writer = reader.into_inner();
                writer
                    .write_all(
                        format!("{}\n", serde_json::to_string(&response).unwrap()).as_bytes(),
                    )
                    .expect("write daemon fixture response");
                writer.flush().expect("flush daemon fixture response");
            }
        });

        let client = Client::new(ClientOptions {
            socket_path: socket,
            session: "default".into(),
            read_timeout: Duration::from_millis(100),
            autostart: false,
            expected_engine: Some("firefox".into()),
            expected_policy: Some(symbrowse_daemon::PolicyStatus {
                ssrf_enabled: true,
                fetch_ssrf_enabled: true,
                allow_private: true,
                ..Default::default()
            }),
            ..Default::default()
        });
        let response = client
            .request(Frame {
                cmd: "tabs.list".into(),
                ..Default::default()
            })
            .expect("config mismatches must not block request");
        assert!(response.success);
        thread.join().unwrap();
        let seen: Vec<_> = seen.try_iter().collect();
        assert_eq!(seen, ["tabs.list", "daemon.status"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dmn006_operation_deadline_cancels_handler_and_keeps_connection_usable() {
        let root = root("late-response");
        let socket = root.join("default.sock");
        let (cancelled_tx, cancelled_rx) = mpsc::channel();
        let server = Arc::new(
            Server::new(ServerOptions {
                socket_path: socket.clone(),
                session: "default".into(),
                idle_timeout: None,
                operation_timeout: Duration::from_millis(25),
                handler: Some(Arc::new(move |frame, operation| {
                    if frame.cmd == "slow" {
                        thread::sleep(Duration::from_millis(50));
                        let _ = cancelled_tx.send(operation.is_cancelled());
                    }
                    Ok((Some(serde_json::json!({"pong": true})), Vec::new()))
                })),
                ..Default::default()
            })
            .unwrap(),
        );
        let running = server.clone();
        let thread = thread::spawn(move || running.listen_and_serve());
        wait_for_socket(&socket);

        let mut stream = std::os::unix::net::UnixStream::connect(&socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        stream
            .write_all(b"{\"cmd\":\"slow\"}\n{\"cmd\":\"daemon.ping\"}\n")
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut first = String::new();
        let mut second = String::new();
        reader.read_line(&mut first).unwrap();
        reader.read_line(&mut second).unwrap();
        let first: serde_json::Value = serde_json::from_str(&first).unwrap();
        let second: serde_json::Value = serde_json::from_str(&second).unwrap();
        assert_eq!(first["error"]["code"], "operation_timeout");
        assert_eq!(
            first["error"]["message"],
            "daemon operation exceeded its timeout"
        );
        assert_eq!(second["data"]["pong"], true);

        assert!(
            cancelled_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("handler did not report cancellation within one second"),
            "operation deadline did not cancel the handler context"
        );
        let mut late = String::new();
        let read = reader.read_line(&mut late);
        assert!(
            read.is_err() || read.unwrap() == 0,
            "late handler completion emitted an unexpected frame: {late:?}"
        );

        server.stop();
        assert!(thread.join().unwrap().is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dmn006_client_disconnect_does_not_cancel_blocked_handler_or_stop_daemon() {
        let root = tempfile::Builder::new()
            .prefix("sb-")
            .tempdir_in(socket_temp_parent())
            .expect("temporary disconnect root");
        let socket = root.path().join("default.sock");
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Arc::new(std::sync::Mutex::new(release_rx));
        let (finished_tx, finished_rx) = mpsc::channel();
        let server = Arc::new(
            Server::new(ServerOptions {
                socket_path: socket.clone(),
                session: "default".into(),
                idle_timeout: None,
                operation_timeout: Duration::from_secs(1),
                handler: Some(Arc::new(move |frame, operation| {
                    if frame.cmd == "blocked" {
                        started_tx
                            .send(operation.clone())
                            .expect("publish blocked operation context");
                        release_rx
                            .lock()
                            .expect("lock release receiver")
                            .recv_timeout(Duration::from_secs(2))
                            .expect("release blocked handler");
                        finished_tx.send(()).expect("publish handler completion");
                    }
                    Ok((Some(serde_json::json!({"pong": true})), Vec::new()))
                })),
                ..Default::default()
            })
            .expect("disconnect fixture server"),
        );
        let running = server.clone();
        let server_thread = thread::spawn(move || running.listen_and_serve());
        wait_for_socket(&socket);

        let mut stream = std::os::unix::net::UnixStream::connect(&socket).unwrap();
        stream
            .write_all(
                format!(
                    "{}\n",
                    serde_json::to_string(&Frame {
                        cmd: "blocked".into(),
                        ..Default::default()
                    })
                    .unwrap()
                )
                .as_bytes(),
            )
            .unwrap();
        let operation = started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("blocked handler started");
        drop(stream);
        thread::sleep(Duration::from_millis(40));
        assert!(
            !operation.is_cancelled(),
            "client disconnect canceled the Rust handler"
        );

        release_tx.send(()).expect("release blocked handler");
        finished_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("blocked handler completed after release");
        let response = Client::new(ClientOptions {
            socket_path: socket.clone(),
            session: "default".into(),
            autostart: false,
            ..Default::default()
        })
        .request_without_autostart(Frame {
            cmd: "daemon.ping".into(),
            ..Default::default()
        })
        .expect("server remains usable after client disconnect");
        assert!(response.success, "response = {response:?}");

        server.stop();
        assert!(server_thread.join().unwrap().is_ok());
    }

    #[test]
    fn production_runtime_cancellation_allows_same_process_restart() {
        let root = std::path::PathBuf::from(format!(
            "/tmp/symbrowse-pc-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let socket = root.join("default.sock");
        let mut spec = SessionSpec::for_session("default");
        spec.socket_path = socket.clone();
        spec.state_dir = root.join("state");
        spec.cache_dir = root.join("cache");
        spec.engine = "static".into();
        spec.mode = "static".into();
        spec.allow_private = true;
        spec.operation_timeout = Duration::from_secs(5);
        spec.read_timeout = Duration::from_millis(200);
        spec.idle_timeout = None;
        let upstream = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let upstream_url = format!("http://{}", upstream.local_addr().unwrap());
        let (request_started, request_ready) = mpsc::channel();
        thread::spawn(move || {
            let Ok((mut stream, _)) = upstream.accept() else {
                return;
            };
            let mut request = [0_u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut request);
            let _ = request_started.send(());
            thread::sleep(Duration::from_secs(5));
        });
        let server = Arc::new(
            Server::new(ServerOptions {
                session_spec: Some(spec.clone()),
                ..Default::default()
            })
            .unwrap(),
        );
        let running = server.clone();
        let thread = thread::spawn(move || running.listen_and_serve());
        thread::sleep(Duration::from_millis(100));
        if thread.is_finished() {
            panic!(
                "production server exited during startup: {:?}",
                thread.join().unwrap()
            );
        }
        wait_for_socket(&socket);
        let request_socket = socket.clone();
        let request = thread::spawn(move || {
            Client::new(ClientOptions {
                socket_path: request_socket,
                session: "default".into(),
                read_timeout: Duration::from_secs(2),
                autostart: false,
                ..Default::default()
            })
            .request_without_autostart(Frame {
                cmd: "open".into(),
                args: Some(serde_json::json!({"url": upstream_url})),
                ..Default::default()
            })
        });
        request_ready
            .recv_timeout(Duration::from_secs(2))
            .expect("upstream request started before cancellation");
        server.stop();
        assert!(thread.join().unwrap().is_ok());
        let request_result = request.join().unwrap();
        match request_result {
            Ok(response) => assert_eq!(response.error.unwrap().code, codes::OPERATION_TIMEOUT),
            Err(ClientError::Transport(error)) => assert_eq!(error.code, "daemon_unavailable"),
            Err(ClientError::Io(error)) => assert!(
                matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::UnexpectedEof
                        | std::io::ErrorKind::NotConnected
                        | std::io::ErrorKind::InvalidInput
                ),
                "unexpected cancellation I/O error: {error:?}"
            ),
            Err(error) => panic!("production cancellation request = {error:?}"),
        }

        let restarted = Arc::new(
            Server::new(ServerOptions {
                session_spec: Some(spec),
                ..Default::default()
            })
            .unwrap(),
        );
        let running = restarted.clone();
        let restart_thread = thread::spawn(move || running.listen_and_serve());
        wait_for_socket(&socket);
        let response = Client::new(ClientOptions {
            socket_path: socket.clone(),
            session: "default".into(),
            autostart: false,
            ..Default::default()
        })
        .request_without_autostart(Frame {
            cmd: "daemon.ping".into(),
            ..Default::default()
        })
        .unwrap();
        assert!(response.success);
        restarted.stop();
        assert!(restart_thread.join().unwrap().is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    fn wait_for_socket(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if fs::symlink_metadata(path)
                .map(|metadata| metadata.permissions().mode() & 0o777 == 0o600)
                .unwrap_or(false)
            {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("secured socket was not created: {}", path.display());
    }

    fn root(name: &str) -> PathBuf {
        // Unix-domain socket paths have a small platform limit. Keep these
        // fixtures under the harness's short external runtime root.
        socket_temp_parent().join(format!(
            "symbrowse-daemon-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn socket_temp_parent() -> PathBuf {
        std::env::var_os("SYMAIRA_EXTERNAL_RUNTIME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"))
    }

    fn shell_quote(path: &Path) -> String {
        format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
    }
}
