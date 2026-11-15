//! Daemon deadlines over a real Unix socket (#772/#779 DMN-006): server read
//! timeout, operation timeout, handlers that ignore cancellation, client
//! disconnects and shutdown, with and without connection reuse.
//!
//! The expectations come from the static Go sources at `57c26c76e`
//! (`browse/internal/daemon/server.go` `serveConn`/`handleLine`, tests
//! `TestOperationTimeoutKeepsConnectionUsable` and
//! `TestClientReadTimeoutOperationTimeout`) and from the frozen Go capture
//! `fixtures/daemon_oracle/go_timeout.json` that `lifecycle.rs` replays:
//!
//! * the read deadline is re-armed before every frame and covers the whole
//!   frame; when it expires the connection closes without a response;
//! * the operation deadline answers `operation_timeout` with the fixed message
//!   and the connection stays usable; the handler is only told to stop through
//!   its context, never waited for;
//! * a client that disappears during an operation is not noticed until the
//!   response write fails; the operation is bounded by its own deadline.
#![deny(unsafe_code)]
#![allow(clippy::result_large_err)]
#![cfg(unix)]

mod common;

use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use symbrowse_daemon::{
    DaemonHandler, Response, Server, ServerOptions, codes, connect_unix, success_response,
};

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Daemon {
    root: PathBuf,
    socket: PathBuf,
    server: Arc<Server>,
    thread: Option<thread::JoinHandle<Result<(), symbrowse_daemon::ServerError>>>,
}

impl Daemon {
    fn start(read: Duration, operation: Duration, handler: DaemonHandler) -> Self {
        let root = std::env::temp_dir().join(format!(
            "sbd-dl-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let socket = root.join("run/default.sock");
        let server = Arc::new(
            Server::new(ServerOptions {
                socket_path: socket.clone(),
                session: "default".to_owned(),
                idle_timeout: None,
                read_timeout: read,
                operation_timeout: operation,
                handler: Some(handler),
                state_key_sources: Some(common::isolated_key_sources()),
                ..Default::default()
            })
            .unwrap(),
        );
        let running = server.clone();
        let thread = Some(thread::spawn(move || running.listen_and_serve()));
        let deadline = Instant::now() + Duration::from_secs(5);
        while connect_unix(&socket, Duration::from_millis(100)).is_err() {
            assert!(Instant::now() < deadline, "daemon did not start");
            thread::sleep(Duration::from_millis(5));
        }
        Self {
            root,
            socket,
            server,
            thread,
        }
    }

    fn connect(&self) -> UnixStream {
        let stream = UnixStream::connect(&self.socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream
    }

    fn stop(&mut self) -> Result<(), symbrowse_daemon::ServerError> {
        self.server.stop();
        self.thread.take().map_or(Ok(()), |t| t.join().unwrap())
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.server.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn frame(cmd: &str) -> Vec<u8> {
    format!("{{\"cmd\":\"{cmd}\"}}\n").into_bytes()
}

fn read_response(reader: &mut impl BufRead) -> Response {
    let mut line = String::new();
    let read = reader.read_line(&mut line).unwrap();
    assert!(read > 0, "connection closed without a response");
    serde_json::from_str(&line).unwrap()
}

/// True when the peer closed the connection without sending anything more.
fn closed_without_response(stream: &mut UnixStream) -> bool {
    let mut byte = [0_u8; 1];
    match stream.read(&mut byte) {
        Ok(0) => true,
        Ok(_) => false,
        Err(error) => matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
        ),
    }
}

fn pong(
    _: symbrowse_daemon::Frame,
    _: symbrowse_daemon::OperationContext,
) -> symbrowse_daemon::HandlerResult {
    Ok((Some(serde_json::json!({"pong": true})), Vec::new()))
}

fn assert_ping_served(daemon: &Daemon) {
    let mut stream = daemon.connect();
    stream.write_all(&frame("daemon.ping")).unwrap();
    let response = read_response(&mut BufReader::new(stream));
    assert!(response.success, "daemon unusable: {response:?}");
    assert_eq!(response.data.unwrap()["pong"], true);
}

#[test]
fn idle_connection_closes_after_read_timeout_without_a_response() {
    let daemon = Daemon::start(
        Duration::from_millis(200),
        Duration::from_secs(5),
        Arc::new(pong),
    );
    let mut stream = daemon.connect();
    let started = Instant::now();
    assert!(closed_without_response(&mut stream));
    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_millis(150) && waited < Duration::from_secs(3),
        "idle close after {waited:?}"
    );
    // The expired connection does not poison the daemon.
    assert_ping_served(&daemon);
}

#[test]
fn read_deadline_is_rearmed_for_every_frame_on_a_reused_connection() {
    let daemon = Daemon::start(
        Duration::from_millis(600),
        Duration::from_secs(5),
        Arc::new(pong),
    );
    let stream = daemon.connect();
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);
    // Total connection age (~900 ms) exceeds the 600 ms read timeout, but no
    // single wait for a frame does: the connection must stay open.
    for _ in 0..3 {
        thread::sleep(Duration::from_millis(300));
        writer.write_all(&frame("daemon.ping")).unwrap();
        assert!(read_response(&mut reader).success);
    }
    // And once the idle gap does exceed it, the connection closes silently.
    let mut raw = reader.into_inner();
    assert!(closed_without_response(&mut raw));
}

#[test]
fn frame_trickled_past_the_read_deadline_closes_without_a_response() {
    let daemon = Daemon::start(
        Duration::from_millis(400),
        Duration::from_secs(5),
        Arc::new(pong),
    );
    let mut stream = daemon.connect();
    let started = Instant::now();
    // One byte per 100 ms never leaves the socket idle for a whole read
    // interval, so only the per-frame deadline can stop it.
    let mut write_failed = false;
    for byte in b"{\"cmd\":\"daemon.ping\"".iter() {
        if stream.write_all(&[*byte]).is_err() {
            write_failed = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
        if started.elapsed() > Duration::from_secs(4) {
            break;
        }
    }
    let _ = write_failed;
    assert!(
        closed_without_response(&mut stream),
        "partial frame answered"
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "trickled frame held the worker for {:?}",
        started.elapsed()
    );
    assert_ping_served(&daemon);
}

#[test]
fn frame_split_within_the_read_deadline_is_served() {
    let daemon = Daemon::start(
        Duration::from_millis(1000),
        Duration::from_secs(5),
        Arc::new(pong),
    );
    let mut stream = daemon.connect();
    stream.write_all(b"{\"cmd\":\"daemon.").unwrap();
    thread::sleep(Duration::from_millis(150));
    stream.write_all(b"ping\"}\n").unwrap();
    assert!(read_response(&mut BufReader::new(stream)).success);
}

#[test]
fn read_deadline_does_not_cut_a_running_operation() {
    // Read timeout 150 ms < handler 500 ms < operation timeout 3 s: the read
    // deadline is re-armed only after the response, so the answer is a
    // success and the connection continues to work.
    let daemon = Daemon::start(
        Duration::from_millis(150),
        Duration::from_secs(3),
        Arc::new(|frame, operation| {
            if frame.cmd == "slow" {
                thread::sleep(Duration::from_millis(500));
            }
            pong(frame, operation)
        }),
    );
    let stream = daemon.connect();
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);
    writer.write_all(&frame("slow")).unwrap();
    let response = read_response(&mut reader);
    assert!(
        response.success,
        "operation cut by read deadline: {response:?}"
    );
    writer.write_all(&frame("daemon.ping")).unwrap();
    assert!(read_response(&mut reader).success);
}

#[test]
fn handler_ignoring_cancellation_is_answered_at_the_operation_deadline() {
    let finished = Arc::new(AtomicBool::new(false));
    let done = finished.clone();
    let daemon = Daemon::start(
        Duration::from_secs(5),
        Duration::from_millis(200),
        Arc::new(move |frame, operation| {
            if frame.cmd == "blocked" {
                // Deaf to `is_cancelled()` for far longer than the deadline.
                thread::sleep(Duration::from_millis(1500));
                done.store(true, Ordering::Release);
                let _ = operation.is_cancelled();
            }
            pong(frame, operation)
        }),
    );
    let stream = daemon.connect();
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);
    let started = Instant::now();
    writer.write_all(&frame("blocked")).unwrap();
    let response = read_response(&mut reader);
    let elapsed = started.elapsed();
    assert!(!response.success);
    let error = response.error.unwrap();
    assert_eq!(error.code, codes::OPERATION_TIMEOUT);
    assert_eq!(error.message, "daemon operation exceeded its timeout");
    assert!(
        elapsed >= Duration::from_millis(150) && elapsed < Duration::from_millis(1000),
        "timeout answered after {elapsed:?}"
    );
    assert!(
        !finished.load(Ordering::Acquire),
        "daemon waited for the blocked handler"
    );

    // Same connection and a second connection are both served while the
    // detached handler is still blocked.
    writer.write_all(&frame("daemon.ping")).unwrap();
    assert!(read_response(&mut reader).success);
    assert_ping_served(&daemon);
    assert!(!finished.load(Ordering::Acquire));

    // The late completion of the blocked handler writes nothing further.
    thread::sleep(Duration::from_millis(1600));
    assert!(finished.load(Ordering::Acquire));
    let mut raw = reader.into_inner();
    raw.set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let mut extra = [0_u8; 1];
    assert!(
        matches!(raw.read(&mut extra), Err(error) if matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        )),
        "late handler completion produced output"
    );
}

#[test]
fn cooperative_handler_observes_cancellation_at_the_deadline() {
    let saw_cancel = Arc::new(AtomicBool::new(false));
    let seen = saw_cancel.clone();
    let daemon = Daemon::start(
        Duration::from_secs(5),
        Duration::from_millis(100),
        Arc::new(move |_, operation| {
            while !operation.is_cancelled() {
                thread::sleep(Duration::from_millis(2));
            }
            seen.store(true, Ordering::Release);
            Ok((None, Vec::new()))
        }),
    );
    let mut stream = daemon.connect();
    stream.write_all(&frame("slow")).unwrap();
    let response = read_response(&mut BufReader::new(stream));
    assert_eq!(response.error.unwrap().code, codes::OPERATION_TIMEOUT);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !saw_cancel.load(Ordering::Acquire) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(2));
    }
    assert!(saw_cancel.load(Ordering::Acquire));
}

#[test]
fn clients_vanishing_mid_operation_do_not_exhaust_the_workers() {
    // 40 abandoned operations exceed the 32 connection workers. Each is bounded
    // by the operation deadline, so the pool recovers and the daemon serves
    // new requests; the abandoned handlers see the deadline as cancellation.
    let started = Arc::new(AtomicUsize::new(0));
    let cancelled = Arc::new(AtomicUsize::new(0));
    let (s, c) = (started.clone(), cancelled.clone());
    let daemon = Daemon::start(
        Duration::from_secs(5),
        Duration::from_millis(250),
        Arc::new(move |frame, operation| {
            if frame.cmd == "slow" {
                s.fetch_add(1, Ordering::AcqRel);
                while !operation.is_cancelled() {
                    thread::sleep(Duration::from_millis(2));
                }
                c.fetch_add(1, Ordering::AcqRel);
            }
            pong(frame, operation)
        }),
    );
    for _ in 0..40 {
        let mut stream = daemon.connect();
        stream.write_all(&frame("slow")).unwrap();
        drop(stream);
    }
    let began = Instant::now();
    assert_ping_served(&daemon);
    assert!(
        began.elapsed() < Duration::from_secs(5),
        "daemon took {:?} to recover",
        began.elapsed()
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while cancelled.load(Ordering::Acquire) < 40 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(started.load(Ordering::Acquire), 40);
    assert_eq!(cancelled.load(Ordering::Acquire), 40);
}

#[test]
fn disconnect_between_pipelined_frames_serves_nothing_after_the_close() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let daemon = Daemon::start(
        Duration::from_secs(5),
        Duration::from_millis(500),
        Arc::new(move |frame, operation| {
            counted.fetch_add(1, Ordering::AcqRel);
            pong(frame, operation)
        }),
    );
    let mut stream = daemon.connect();
    stream.write_all(&frame("one")).unwrap();
    drop(stream);
    thread::sleep(Duration::from_millis(200));
    // Only the frame that was fully sent was dispatched; a vanished client is
    // not an error for the daemon and the next client is served normally.
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert_ping_served(&daemon);
    assert_eq!(calls.load(Ordering::Acquire), 1);
}

#[test]
fn shutdown_cancels_a_blocked_handler_and_closes_its_connection() {
    let saw_cancel = Arc::new(AtomicBool::new(false));
    let seen = saw_cancel.clone();
    let mut daemon = Daemon::start(
        Duration::from_secs(5),
        Duration::from_secs(60),
        Arc::new(move |_, operation| {
            while !operation.is_cancelled() {
                thread::sleep(Duration::from_millis(2));
            }
            seen.store(true, Ordering::Release);
            Ok((None, Vec::new()))
        }),
    );
    let mut stream = daemon.connect();
    stream.write_all(&frame("slow")).unwrap();
    thread::sleep(Duration::from_millis(100));
    let started = Instant::now();
    assert!(daemon.stop().is_ok());
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "shutdown waited {:?} for a 60 s operation",
        started.elapsed()
    );
    // Shutdown is not an operation timeout. Like Go's `Close` (handlers may
    // finish their current frame), a handler that returns once cancelled can
    // still deliver its own result; otherwise the connection closes without a
    // response. Either way no `operation_timeout` is invented and the
    // connection ends.
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) > 0 {
        let response: Response = serde_json::from_str(&line).unwrap();
        assert_ne!(
            response.error.map(|error| error.code),
            Some(codes::OPERATION_TIMEOUT.to_owned())
        );
        line.clear();
    }
    assert_eq!(reader.read_line(&mut line).unwrap_or(0), 0);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !saw_cancel.load(Ordering::Acquire) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(2));
    }
    assert!(saw_cancel.load(Ordering::Acquire));
}

#[test]
fn shutdown_does_not_wait_for_a_handler_that_ignores_cancellation() {
    let mut daemon = Daemon::start(
        Duration::from_secs(5),
        Duration::from_secs(60),
        Arc::new(|_, _| {
            thread::sleep(Duration::from_secs(10));
            Ok((None, Vec::new()))
        }),
    );
    let mut stream = daemon.connect();
    stream.write_all(&frame("blocked")).unwrap();
    thread::sleep(Duration::from_millis(100));
    let started = Instant::now();
    assert!(daemon.stop().is_ok());
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "shutdown blocked {:?} behind a deaf handler",
        started.elapsed()
    );
    assert!(closed_without_response(&mut stream));
}

#[test]
fn success_response_helper_matches_the_wire_shape_of_a_served_ping() {
    // Guards the fixture assumption used above: handler success data is
    // forwarded verbatim in the standard response envelope.
    let daemon = Daemon::start(
        Duration::from_secs(5),
        Duration::from_secs(5),
        Arc::new(pong),
    );
    let mut stream = daemon.connect();
    stream.write_all(&frame("custom")).unwrap();
    let response = read_response(&mut BufReader::new(stream));
    let expected = success_response(Some(serde_json::json!({"pong": true})), Vec::new());
    assert_eq!(
        serde_json::to_value(&response).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
}
