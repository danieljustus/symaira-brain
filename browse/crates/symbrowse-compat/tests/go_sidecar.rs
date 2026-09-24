use std::{
    io::{self, BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    thread,
    time::Duration,
};

use symbrowse_compat::{CompatClient, CompatError, Request};

fn sidecar(timeout: Duration) -> CompatClient {
    let binary = std::env::var("SYMBROWSE_COMPAT_BINARY")
        .expect("SYMBROWSE_COMPAT_BINARY must point to the native Go executable");
    CompatClient::new(binary, ["compat-sidecar".to_owned()], timeout)
}

fn read_request_line(stream: &mut TcpStream) -> String {
    let mut reader = BufReader::new(stream);
    let mut first = String::new();
    reader.read_line(&mut first).expect("read request line");
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).expect("read request header");
        if header == "\r\n" || header.is_empty() {
            break;
        }
    }
    first
}

fn write_response(stream: &mut TcpStream, body: &str) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn is_expected_closed_peer(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::NotConnected
    )
}

fn request(url: String, profile: &str) -> Request {
    Request {
        id: 0,
        method: "GET".into(),
        url,
        profile: profile.into(),
        headers: vec![("X-Compat-Test".into(), "isolated".into())],
        body: String::new(),
        timeout_ms: 10_000,
        max_body_bytes: 1024,
    }
}

#[tokio::test]
#[ignore = "requires the native Go compat executable"]
async fn production_go_sidecar_exchanges_six_profiles_and_restarts_after_eof() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback fixture");
    let address = listener.local_addr().expect("fixture address");
    let server = thread::spawn(move || {
        for _ in 0..7 {
            let (mut stream, _) = listener.accept().expect("accept Go request");
            let line = read_request_line(&mut stream);
            assert!(line.starts_with("GET /compat HTTP/1.1"), "{line:?}");
            write_response(&mut stream, "compat").expect("write loopback response");
        }
    });

    let profiles = ["chrome", "edge", "firefox", "ios", "opera", "safari"];
    let mut client = sidecar(Duration::from_secs(15));
    for (index, profile) in profiles.iter().enumerate() {
        let response = client
            .request_with_timeout(request(format!("http://{address}/compat"), profile))
            .await
            .expect("Go sidecar response");
        assert!(response.ok, "{response:?}");
        assert_eq!(response.id, index as u64 + 1);
        assert_eq!(response.status, 200);
        assert_eq!(response.body, "compat");
    }

    // EOF is the protocol shutdown; the next request starts a fresh process
    // while request IDs remain monotonic for the client instance.
    client.shutdown().await.expect("Go sidecar exits on EOF");
    let response = client
        .request_with_timeout(request(format!("http://{address}/compat"), "chrome"))
        .await
        .expect("sidecar restarts after clean exit");
    assert!(response.ok, "{response:?}");
    assert_eq!(response.id, 7);
    assert_eq!(response.body, "compat");
    client
        .shutdown()
        .await
        .expect("restarted Go sidecar exits on EOF");
    server.join().expect("fixture server");
}

#[tokio::test]
#[ignore = "requires the native Go compat executable"]
async fn production_go_sidecar_returns_a_typed_fetch_error() {
    let mut client = sidecar(Duration::from_secs(10));
    let response = client
        .request_with_timeout(request("not-a-valid-url".into(), "chrome"))
        .await
        .expect("a fetch error is a protocol response, not a process failure");
    assert!(!response.ok, "{response:?}");
    let error = response.error.expect("typed sidecar error");
    assert_eq!(error.code, "compat_fetch_failed");
    assert!(!error.retryable);
    client.shutdown().await.expect("Go sidecar exits on EOF");
}

#[tokio::test]
#[ignore = "requires the native Go compat executable"]
async fn production_go_sidecar_discards_late_response_after_timeout_restart() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback fixture");
    let address = listener.local_addr().expect("fixture address");
    let (accepted_tx, accepted_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let mut workers = Vec::new();
        for index in 0..2 {
            let (mut stream, _) = listener.accept().expect("accept Go request");
            let line = read_request_line(&mut stream);
            assert!(
                line.starts_with("GET /slow HTTP/1.1") || line.starts_with("GET /fast HTTP/1.1"),
                "{line:?}"
            );
            if index == 0 {
                accepted_tx
                    .send(())
                    .expect("signal first request reached fixture");
            }
            workers.push(thread::spawn(move || {
                if index == 0 {
                    thread::sleep(Duration::from_secs(3));
                }
                if let Err(error) =
                    write_response(&mut stream, if index == 0 { "late" } else { "fast" })
                {
                    assert!(
                        index == 0 && is_expected_closed_peer(&error),
                        "unexpected loopback response write failure: {error}"
                    );
                }
            }));
        }
        for worker in workers {
            worker.join().expect("fixture connection worker");
        }
    });

    let mut client = sidecar(Duration::from_secs(2));
    let first = tokio::spawn(async move {
        let result = client
            .request_with_timeout(request(format!("http://{address}/slow"), "chrome"))
            .await;
        (client, result)
    });
    tokio::task::spawn_blocking(move || accepted_rx.recv_timeout(Duration::from_secs(10)))
        .await
        .expect("fixture waiter task")
        .expect("first request reached the local fixture");
    let (mut client, timed_out) = first.await.expect("timed-out request task");
    assert!(matches!(timed_out, Err(CompatError::Timeout)));

    let response = client
        .request_with_timeout(request(format!("http://{address}/fast"), "chrome"))
        .await
        .expect("new sidecar handles request after timeout");
    assert!(response.ok, "{response:?}");
    assert_eq!(response.id, 2);
    assert_eq!(response.body, "fast");
    client
        .shutdown()
        .await
        .expect("restarted Go sidecar exits on EOF");
    server.join().expect("fixture server");
}
