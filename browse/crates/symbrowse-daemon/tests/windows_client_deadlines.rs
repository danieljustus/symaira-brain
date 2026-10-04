#![cfg(windows)]
#![deny(unsafe_code)]

use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Barrier, mpsc},
    thread,
    time::{Duration, Instant},
};
use symbrowse_daemon::{Client, ClientError, ClientOptions, Frame, MAX_FRAME_BYTES, codes};
use tokio::net::windows::named_pipe::{ClientOptions as PipeClientOptions, ServerOptions};

const SCENARIO: &str = "SYMBROWSE_OWNED_CLIENT_SCENARIO";

#[path = "support/windows_endpoint_relation.rs"]
mod endpoint_relation;

#[test]
#[ignore = "mandatory native Windows CI; external owned-child watchdog"]
fn native_windows_client_deadlines() {
    let parent = std::env::var_os("RUNNER_TEMP")
        .map_or_else(std::env::temp_dir, std::path::PathBuf::from)
        .join("browse-daemon-client-deadlines")
        .join(std::process::id().to_string());
    fs::create_dir_all(&parent).unwrap();
    for scenario in [
        "unread-request",
        "full-request",
        "busy-instance",
        "eight-clients",
        "endpoint-ascii",
        "endpoint-unicode",
        "endpoint-lossy",
    ] {
        let root = parent.join(scenario);
        fs::create_dir(&root).unwrap();
        let stdout = root.join("stdout.bin");
        let stderr = root.join("stderr.bin");
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "native_windows_client_owned_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env_clear()
            .env(SCENARIO, scenario)
            .env("HOME", &root)
            .env("USERPROFILE", &root)
            .env("LOCALAPPDATA", root.join("local"))
            .env("XDG_CACHE_HOME", root.join("cache"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_STATE_HOME", root.join("state"))
            .env("TMP", &root)
            .env("TEMP", &root)
            .env("PATH", "")
            .current_dir(&root)
            .stdin(Stdio::null())
            .stdout(Stdio::from(fs::File::create(&stdout).unwrap()))
            .stderr(Stdio::from(fs::File::create(&stderr).unwrap()));
        for name in ["SystemRoot", "windir"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        let began = Instant::now();
        let mut child = command.spawn().unwrap();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break Some(status);
            }
            if began.elapsed() >= Duration::from_secs(8) {
                break None;
            }
            thread::sleep(Duration::from_millis(10));
        };
        if status.is_none() {
            child.kill().unwrap();
            let cleanup = Instant::now();
            while child.try_wait().unwrap().is_none() && cleanup.elapsed() < Duration::from_secs(2)
            {
                thread::sleep(Duration::from_millis(10));
            }
        }
        let observed = serde_json::json!({"scenario": scenario, "pid": child.id(),
            "seconds": began.elapsed().as_secs_f64(), "deadline_seconds": 8,
            "timed_out": status.is_none(), "exit": child.try_wait().unwrap().map(|s| s.to_string()),
            "stdout": stdout, "stderr": stderr});
        fs::write(
            root.join("result.json"),
            serde_json::to_vec_pretty(&observed).unwrap(),
        )
        .unwrap();
        eprintln!(
            "owned fixture: {observed}; stdout={:?}; stderr={:?}",
            fs::read(&stdout).unwrap(),
            fs::read(&stderr).unwrap()
        );
        assert!(
            status.is_some_and(|status| status.success()),
            "client owner failed: {observed}"
        );
    }
}

#[test]
#[ignore = "only the mandatory watchdog launches this owned fixture"]
fn native_windows_client_owned_fixture() {
    let scenario = std::env::var(SCENARIO).expect("owned fixture scenario required");
    let session = format!("client-{}-{}", std::process::id(), scenario);
    let endpoint = symbrowse_daemon::default_socket_path(&session);
    eprintln!("phase=begin scenario={scenario} endpoint={endpoint:?}");
    if scenario.starts_with("endpoint-") {
        endpoint_relation::run(&endpoint, &session, &scenario);
    } else if scenario == "eight-clients" {
        eight_clients(&endpoint, &session);
    } else {
        blocked_peer(&endpoint, &session, &scenario);
    }
    eprintln!("phase=finished scenario={scenario}");
}

fn options(endpoint: &Path, session: &str, timeout: Duration) -> ClientOptions {
    ClientOptions {
        socket_path: endpoint.into(),
        session: session.into(),
        read_timeout: timeout,
        startup_timeout: Duration::from_millis(200),
        autostart: false,
        start: None,
        expected_engine: None,
        expected_policy: None,
    }
}

fn blocked_peer(endpoint: &Path, session: &str, scenario: &str) {
    let (ready, readiness) = mpsc::channel();
    let (accepted, connection) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let path = endpoint.to_owned();
    let peer = thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let pipe = ServerOptions::new()
                .first_pipe_instance(true)
                .max_instances(1)
                .in_buffer_size(512)
                .out_buffer_size(512)
                .create(&path)
                .unwrap();
            ready.send(pipe.info().unwrap().in_buffer_size).unwrap();
            pipe.connect().await.unwrap();
            accepted.send(()).unwrap();
            // Deliberately hold an unread request until the client has returned.
            // The external process owns this peer and its watchdog kills all its
            // threads on failure; no test harness thread must join a stuck client.
            released.recv_timeout(Duration::from_secs(6)).unwrap();
            drop(pipe);
        });
    });
    let capacity = readiness.recv_timeout(Duration::from_secs(2)).unwrap();
    let holder_runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let holder = if scenario == "busy-instance" {
        let entered = holder_runtime.enter();
        let stream = PipeClientOptions::new().open(endpoint).unwrap();
        drop(entered);
        connection.recv_timeout(Duration::from_secs(2)).unwrap();
        Some(stream)
    } else {
        None
    };
    let args = (scenario == "full-request").then(|| {
        let payload = "x".repeat(MAX_FRAME_BYTES / 2);
        assert!(
            payload.len() > capacity as usize,
            "fixture must exceed native advertised input capacity"
        );
        serde_json::json!({"owned": payload})
    });
    let frame = Frame {
        cmd: "daemon.ping".into(),
        args,
        ..Frame::default()
    };
    assert!(serde_json::to_vec(&frame).unwrap().len() < MAX_FRAME_BYTES);
    let began = Instant::now();
    eprintln!("phase=request capacity={capacity}");
    let error = Client::new(options(endpoint, session, Duration::from_millis(150)))
        .request_without_autostart(frame)
        .expect_err("unread/busy peer must not complete successfully");
    eprintln!(
        "phase=client-return elapsed={:?} error={error}",
        began.elapsed()
    );
    assert!(
        began.elapsed() < Duration::from_secs(2),
        "deadline also bounds client runtime drop"
    );
    let ClientError::Transport(error) = error else {
        panic!("expected stable transport error")
    };
    assert_eq!(
        error.code,
        if scenario == "busy-instance" {
            codes::DAEMON_UNAVAILABLE
        } else {
            codes::OPERATION_TIMEOUT
        }
    );
    if holder.is_none() {
        connection.recv_timeout(Duration::from_secs(2)).unwrap();
    }
    release.send(()).unwrap();
    drop(holder);
    drop(holder_runtime);
    peer.join().unwrap();
}

// The public handler ABI returns DaemonError by value, as in platform.rs.
#[allow(clippy::result_large_err)]
fn eight_clients(endpoint: &Path, session: &str) {
    let server = Arc::new(
        symbrowse_daemon::Server::new(symbrowse_daemon::ServerOptions {
            socket_path: endpoint.into(),
            session: session.into(),
            idle_timeout: None,
            handler: Some(Arc::new(|frame, _| {
                Ok((
                    Some(serde_json::json!({"request": frame.request_id})),
                    Vec::new(),
                ))
            })),
            ..Default::default()
        })
        .unwrap(),
    );
    let owner = server.clone();
    let serving = thread::spawn(move || owner.listen_and_serve());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let entered = runtime.enter();
        let opened = PipeClientOptions::new().open(endpoint);
        drop(entered);
        if let Ok(stream) = opened {
            drop(stream);
            break;
        }
        assert!(
            !serving.is_finished() && Instant::now() < deadline,
            "owned daemon did not become ready"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let barrier = Arc::new(Barrier::new(8));
    let clients = (0..8)
        .map(|index| {
            let barrier = barrier.clone();
            let opts = options(endpoint, session, Duration::from_secs(2));
            thread::spawn(move || {
                barrier.wait();
                let id = format!("owned-{index}");
                let response = Client::new(opts)
                    .request_without_autostart(Frame {
                        cmd: "owned.echo".into(),
                        request_id: id.clone(),
                        ..Frame::default()
                    })
                    .unwrap();
                assert!(response.success);
                assert_eq!(response.data, Some(serde_json::json!({"request": id})));
            })
        })
        .collect::<Vec<_>>();
    for client in clients {
        client.join().unwrap();
    }
    server.stop();
    assert!(serving.join().unwrap().is_ok());
    drop(runtime);
}
