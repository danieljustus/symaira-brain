#![cfg(feature = "test-fakemcp")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::value::RawValue;
use symbrain_broker::{BrokerError, Client, Config, ManagedServer, Options, State};

#[derive(Deserialize)]
struct Oracle {
    tool_names: Vec<String>,
    echo_text: String,
    tool_error: bool,
    tool_error_text: String,
    timeout_category: String,
    mismatch_category: String,
    mismatch_state: String,
    crash_spawn_count: usize,
    crash_state: String,
}

fn oracle() -> Oracle {
    serde_json::from_slice(include_bytes!("fixtures/oracle_expectations.json"))
        .expect("parse broker oracle")
}

fn fakemcp_path() -> &'static str {
    env!("CARGO_BIN_EXE_symbrain-fakemcp")
}

fn cfg() -> Config {
    Config {
        name: "test".to_string(),
        binary_path: fakemcp_path().to_string(),
        init_timeout: Duration::from_secs(5),
        max_restarts: 1,
        backoff_base: Duration::from_millis(10),
        shutdown_timeout: Duration::from_millis(500),
        ..Config::default()
    }
}

fn cfg_with_env(values: &[(&str, &str)]) -> Config {
    let mut config = cfg();
    config.env = Some(
        values
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect(),
    );
    config
}

fn wait_until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !predicate() {
        assert!(Instant::now() < deadline, "condition timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn initialize_list_tools_and_call_roundtrip() {
    let server = ManagedServer::new(cfg());
    let tools = server.list_tools().expect("list tools");
    let mut names = tools
        .iter()
        .map(|tool| tool.name.clone())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, oracle().tool_names);

    let arguments = RawValue::from_string(r#"{"x":1}"#.to_string()).expect("raw arguments");
    let result = server
        .call_tool("echo", Some(arguments.as_ref()))
        .expect("call echo");
    assert!(!result.is_error);
    assert_eq!(result.content[0].text, oracle().echo_text);

    let tool_error = server.call_tool("toolerror", None).expect("tool result");
    assert_eq!(tool_error.is_error, oracle().tool_error);
    assert_eq!(tool_error.content[0].text, oracle().tool_error_text);

    server.shutdown();
    assert_eq!(server.state(), State::Stopped);
}

#[test]
fn slow_call_times_out_and_shutdown_reaps_child() {
    let mut config = cfg_with_env(&[("FAKEMCP_SLOW_MS", "500")]);
    config.call_timeout = Duration::from_millis(20);
    let server = ManagedServer::new(config);
    let _ = server.list_tools().expect("list tools");
    let error = server
        .call_tool("slow", None)
        .expect_err("slow call must time out");
    assert!(matches!(error, BrokerError::Timeout { .. }));
    assert_eq!(oracle().timeout_category, "timeout");
    server.shutdown();
    assert_eq!(server.state(), State::Stopped);
}

#[test]
fn in_flight_child_call_observes_connection_cancellation() {
    let mut config = cfg_with_env(&[("FAKEMCP_SLOW_MS", "2000")]);
    config.call_timeout = Duration::from_secs(5);
    let server = Arc::new(ManagedServer::new(config));
    let _ = server.list_tools().expect("list tools");
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_server = Arc::clone(&server);
    let worker_cancelled = Arc::clone(&cancelled);
    let started = Instant::now();
    let worker = std::thread::spawn(move || {
        worker_server
            .call_tool_with_cancel("slow", None, &|| worker_cancelled.load(Ordering::Acquire))
    });
    std::thread::sleep(Duration::from_millis(50));
    cancelled.store(true, Ordering::Release);

    let error = worker
        .join()
        .expect("join cancellation worker")
        .expect_err("cancelled call must fail");
    assert!(matches!(error, BrokerError::Cancelled { .. }));
    assert!(started.elapsed() < Duration::from_secs(1));
    server.shutdown();
}

#[cfg(unix)]
#[test]
fn client_drop_kills_process_group_descendants() {
    use std::fs;

    let dir = tempfile::tempdir().expect("tempdir");
    let pid_file = dir.path().join("descendant.pid");
    let client = Client::spawn(
        "/bin/sh",
        Options {
            args: vec![
                "-c".to_string(),
                "sleep 30 & printf '%s' \"$!\" > \"$1\"; sleep 30".to_string(),
                "probe".to_string(),
                pid_file.display().to_string(),
            ],
            ..Options::default()
        },
    )
    .expect("spawn client");
    let deadline = Instant::now() + Duration::from_secs(1);
    while !pid_file.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let pid = fs::read_to_string(&pid_file).expect("descendant pid");
    drop(client);

    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let alive = std::process::Command::new("/bin/kill")
            .args(["-0", pid.trim()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if !alive {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "descendant {pid} survived client drop"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn captures_child_stderr_without_stdout_pollution() {
    let mut config = cfg_with_env(&[("FAKEMCP_STDERR", "diagnostic-only")]);
    config.capture_stderr = true;
    let server = ManagedServer::new(config);
    let _ = server.list_tools().expect("list tools");
    wait_until(|| String::from_utf8_lossy(&server.stderr_bytes()).contains("diagnostic-only"));
    server.shutdown();
}

#[test]
fn protocol_mismatch_is_terminal_degradation() {
    let server = ManagedServer::new(cfg_with_env(&[("FAKEMCP_PROTOCOL_VERSION", "1900-01-01")]));
    let error = server.list_tools().expect_err("mismatch must fail");
    assert!(matches!(error, BrokerError::ProtocolMismatch { .. }));
    assert_eq!(oracle().mismatch_category, "protocol_mismatch");
    assert_eq!(server.state().as_str(), oracle().mismatch_state);
    assert!(server.last_error().is_some());
    server.shutdown();
}

#[test]
fn crash_then_restart_with_backoff() {
    let marker_dir = tempfile::tempdir().expect("tempdir");
    let marker = marker_dir.path().join("spawns.txt");
    let server = ManagedServer::new(cfg_with_env(&[(
        "FAKEMCP_SPAWN_MARKER",
        marker.to_str().expect("utf8 path"),
    )]));
    let _ = server.list_tools().expect("list tools");
    assert!(server.call_tool("crash", None).is_err());

    wait_until(|| {
        std::fs::read_to_string(&marker).is_ok_and(|content| content.lines().count() >= 2)
    });
    let tools = server.list_tools().expect("list after restart");
    assert!(!tools.is_empty());
    assert_eq!(read_pids(&marker).len(), oracle().crash_spawn_count);
    assert_eq!(server.state().as_str(), oracle().crash_state);

    server.shutdown();
    let pids = read_pids(&marker);
    wait_until(|| pids.iter().all(|pid| !process_alive(*pid)));
}

#[test]
fn concurrent_lazy_start_spawns_exactly_one_child() {
    let marker_dir = tempfile::tempdir().expect("tempdir");
    let marker = marker_dir.path().join("spawns.txt");
    let server = Arc::new(ManagedServer::new(cfg_with_env(&[(
        "FAKEMCP_SPAWN_MARKER",
        marker.to_str().expect("utf8 path"),
    )])));
    let barrier = Arc::new(Barrier::new(8));
    let threads = (0..8)
        .map(|_| {
            let server = Arc::clone(&server);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                server.list_tools().expect("concurrent list")
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        assert!(!thread.join().expect("join").is_empty());
    }
    assert_eq!(read_pids(&marker).len(), 1, "duplicate child spawn");
    server.shutdown();
}

#[cfg(unix)]
#[test]
fn shutdown_kills_descendant_process_group() {
    let marker_dir = tempfile::tempdir().expect("tempdir");
    let marker = marker_dir.path().join("descendant.txt");
    let server = ManagedServer::new(cfg_with_env(&[(
        "FAKEMCP_DESCENDANT_MARKER",
        marker.to_str().expect("utf8 path"),
    )]));
    let _ = server.list_tools().expect("list tools");
    wait_until(|| marker.exists());
    let pid = std::fs::read_to_string(&marker)
        .expect("read marker")
        .trim()
        .parse::<u32>()
        .expect("parse pid");
    assert!(
        process_alive(pid),
        "descendant should be alive before shutdown"
    );

    server.shutdown();
    wait_until(|| !process_alive(pid));
    assert_eq!(server.state(), State::Stopped);
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    std::process::Command::new("/bin/kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    false
}

fn read_pids(path: &std::path::Path) -> Vec<u32> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.parse().ok())
        .collect()
}
