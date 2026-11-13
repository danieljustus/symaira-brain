//! Frame decoding and size limits of the Unix socket server (#772 DMN-002).
//!
//! Expectations are derived from the static Go sources at `57c26c76e`
//! (`browse/internal/daemon/protocol.go` `DecodeFrame`, `server.go`
//! `serveConn`/`handleLine`): an undecodable or command-less frame gets a
//! `malformed_request` response and the connection stays usable; a line that
//! does not fit the 1 MiB scanner buffer (`bufio.Scanner` with
//! `maxFrameBytes = 1 << 20`) closes the connection without any response.
#![deny(unsafe_code)]
#![cfg(unix)]

mod common;

use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use symbrowse_daemon::{MAX_FRAME_BYTES, Response, Server, ServerOptions, codes, connect_unix};

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Daemon {
    root: PathBuf,
    socket: PathBuf,
    server: Arc<Server>,
    thread: Option<thread::JoinHandle<Result<(), symbrowse_daemon::ServerError>>>,
}

impl Daemon {
    fn start() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sbd-frames-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let socket = root.join("run/default.sock");
        let server = Arc::new(
            Server::new(ServerOptions {
                socket_path: socket.clone(),
                session: "default".to_owned(),
                idle_timeout: None,
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

    /// One request line on a fresh connection; the response line is returned.
    fn exchange(&self, line: &[u8]) -> Response {
        let mut stream = self.connect();
        stream.write_all(line).unwrap();
        stream.write_all(b"\n").unwrap();
        read_response(&mut BufReader::new(stream))
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

fn read_response(reader: &mut impl BufRead) -> Response {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap_or_else(|error| panic!("response {line:?}: {error}"))
}

fn error_of(response: &Response) -> (&str, &str) {
    assert!(!response.success, "{response:?}");
    let error = response.error.as_ref().expect("error payload");
    (error.code.as_str(), error.message.as_str())
}

/// A valid ping frame whose JSON text is exactly `length` bytes.
fn ping_frame(length: usize) -> Vec<u8> {
    let frame = |padding: usize| {
        format!(
            r#"{{"cmd":"daemon.ping","args":"{}"}}"#,
            "x".repeat(padding)
        )
        .into_bytes()
    };
    let base = frame(0).len();
    let padded = frame(length - base);
    assert_eq!(padded.len(), length);
    padded
}

/// True when the daemon closed the connection without sending any response.
fn closed_without_response(mut stream: UnixStream) -> bool {
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

#[test]
fn undecodable_and_command_less_frames_answer_malformed_request_and_keep_the_connection() {
    let daemon = Daemon::start();
    let mut stream = daemon.connect();
    let cases: [(&[u8], &str); 8] = [
        (br#"{"session":"default"}"#, "missing cmd"),
        (br#"{"cmd":""}"#, "missing cmd"),
        (b"{}", "missing cmd"),
        (b"not json", "decode frame: "),
        (br#"{"cmd":1}"#, "decode frame: "),
        (
            b"[]",
            "decode frame: json: cannot unmarshal array into Go value of type daemon.Frame",
        ),
        (
            b"[\"daemon.ping\"]",
            "decode frame: json: cannot unmarshal array into Go value of type daemon.Frame",
        ),
        (b"null", "missing cmd"),
    ];
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    for (line, message) in cases {
        stream.write_all(line).unwrap();
        stream.write_all(b"\n").unwrap();
        let response = read_response(&mut reader);
        let (code, text) = error_of(&response);
        assert_eq!(code, codes::MALFORMED_REQUEST, "line={line:?}");
        // Go's decode errors are encoding/json texts; only their stable
        // `decode frame: ` prefix is part of this port's contract.
        if message == "missing cmd" || message.contains("Go value") {
            assert_eq!(text, message, "line={line:?}");
        } else {
            assert!(text.starts_with(message), "line={line:?} text={text:?}");
        }
    }
    stream.write_all(b"{\"cmd\":\"daemon.ping\"}\n").unwrap();
    let response = read_response(&mut reader);
    assert!(response.success, "{response:?}");
    assert_eq!(response.data.unwrap()["pong"], true);
}

#[test]
fn whitespace_only_command_is_a_command_not_a_missing_one() {
    let daemon = Daemon::start();
    let response = daemon.exchange(br#"{"cmd":" "}"#);
    assert!(!response.success);
    assert_ne!(
        error_of(&response).0,
        codes::MALFORMED_REQUEST,
        "{response:?}"
    );
}

#[test]
fn frame_of_one_mib_minus_one_is_served_and_one_mib_closes_the_connection() {
    let daemon = Daemon::start();
    // `bufio.Scanner` with a 1 MiB buffer holds the line and its newline.
    let largest = ping_frame(MAX_FRAME_BYTES - 1);
    let response = daemon.exchange(&largest);
    assert!(response.success, "{response:?}");
    assert_eq!(response.data.unwrap()["pong"], true);

    let mut stream = daemon.connect();
    let _ = stream.write_all(&ping_frame(MAX_FRAME_BYTES));
    let _ = stream.write_all(b"\n");
    assert!(
        closed_without_response(stream),
        "a 1 MiB line must be dropped silently"
    );
    // The daemon itself keeps serving.
    assert!(daemon.exchange(b"{\"cmd\":\"daemon.ping\"}").success);
}

#[test]
fn oversized_line_without_newline_closes_the_connection() {
    let daemon = Daemon::start();
    let mut stream = daemon.connect();
    let chunk = vec![b'x'; 64 * 1024];
    for _ in 0..(2 * MAX_FRAME_BYTES / chunk.len()) {
        if stream.write_all(&chunk).is_err() {
            break;
        }
    }
    assert!(
        closed_without_response(stream),
        "an unterminated oversized line must be dropped silently"
    );
    assert!(daemon.exchange(b"{\"cmd\":\"daemon.ping\"}").success);
}

#[test]
fn oversized_line_after_a_served_frame_closes_only_that_connection() {
    let daemon = Daemon::start();
    let mut stream = daemon.connect();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    stream.write_all(b"{\"cmd\":\"daemon.ping\"}\n").unwrap();
    assert!(read_response(&mut reader).success);
    let _ = stream.write_all(&vec![b'y'; MAX_FRAME_BYTES + 1]);
    let _ = stream.write_all(b"\n");
    let mut rest = Vec::new();
    let _ = reader.read_to_end(&mut rest);
    assert!(
        rest.is_empty(),
        "unexpected bytes after the oversized line: {rest:?}"
    );
}
