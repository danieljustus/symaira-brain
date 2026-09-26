use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use serde_json::json;
use symbrowse_daemon::{Client, ClientOptions, Frame, Server, ServerOptions, SessionSpec};
use symbrowse_engine_firefox::{
    FirefoxError, FirefoxSession, canonical_capabilities, resolve_firefox_executable,
};

fn daemon_request(client: &Client, command: &str, args: serde_json::Value) -> serde_json::Value {
    let response = client
        .request(Frame {
            cmd: command.into(),
            args: Some(args),
            session: client.options().session.clone(),
            ..Frame::default()
        })
        .unwrap_or_else(|error| panic!("Firefox daemon request {command}: {error}"));
    serde_json::to_value(response).expect("serialize Firefox daemon response")
}

fn production_daemon_uses_the_selected_firefox(executable: PathBuf, fixture_url: &str) {
    let root = tempfile::tempdir().expect("create Firefox daemon root");
    let session = format!("native-firefox-{}", std::process::id());
    let mut spec = SessionSpec::for_session(&session);
    spec.engine = "firefox".into();
    spec.mode = "browser".into();
    spec.executable_path = executable;
    spec.state_dir = root.path().join("state");
    spec.cache_dir = root.path().join("cache");
    spec.daemon_log = root.path().join("daemon.log");
    #[cfg(target_os = "macos")]
    let socket_dir = tempfile::Builder::new()
        .prefix("sbfox-")
        .tempdir_in("/private/tmp")
        .expect("create private short socket directory");
    #[cfg(target_os = "macos")]
    {
        spec.socket_path = socket_dir.path().join("daemon.sock");
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        spec.socket_path = root.path().join("daemon.sock");
    }

    let socket = spec.socket_path.clone();
    let server = std::sync::Arc::new(
        Server::new(ServerOptions {
            session_spec: Some(spec),
            ..ServerOptions::default()
        })
        .expect("create Firefox daemon"),
    );
    let serving = std::sync::Arc::clone(&server);
    let thread = thread::spawn(move || serving.listen_and_serve().expect("serve Firefox daemon"));
    let client = Client::new(ClientOptions {
        socket_path: socket,
        session: session.clone(),
        read_timeout: Duration::from_secs(45),
        startup_timeout: Duration::from_secs(5),
        autostart: false,
        expected_engine: Some("firefox".into()),
        ..ClientOptions::default()
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if client
            .request(Frame {
                cmd: "daemon.status".into(),
                session: session.clone(),
                ..Frame::default()
            })
            .is_ok()
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Firefox daemon endpoint did not become ready"
        );
        thread::sleep(Duration::from_millis(20));
    }

    let capabilities = daemon_request(&client, "capabilities", json!({}));
    assert_eq!(
        capabilities["success"], true,
        "capabilities: {capabilities}"
    );
    assert_eq!(capabilities["data"]["kind"], "firefox");
    let opened = daemon_request(&client, "open", json!({"url":fixture_url}));
    assert_eq!(opened["success"], true, "open: {opened}");
    assert_eq!(opened["data"]["title"], "Firefox fixture");
    let evaluated = daemon_request(&client, "eval", json!({"expression":"document.title"}));
    assert_eq!(evaluated["success"], true, "eval: {evaluated}");
    assert_eq!(evaluated["data"]["value"], "Firefox fixture");
    let unsupported = daemon_request(&client, "download", json!({}));
    assert_eq!(unsupported["success"], false, "download: {unsupported}");
    assert_eq!(unsupported["error"]["code"], "unsupported");

    server.stop();
    thread.join().expect("join Firefox daemon");
}

struct FixtureServer {
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl FixtureServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind Firefox fixture");
        listener
            .set_nonblocking(true)
            .expect("make fixture listener nonblocking");
        let address = listener.local_addr().expect("fixture address");
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => serve_fixture_request(&mut stream),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            address,
            stop,
            thread: Some(thread),
        }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve_fixture_request(stream: &mut TcpStream) {
    stream
        .set_nonblocking(false)
        .expect("make accepted Firefox fixture connection blocking");
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut request_line = String::new();
    let _ = BufReader::new(&mut *stream).read_line(&mut request_line);
    eprintln!("firefox_fixture_request={}", request_line.trim_end());
    if request_line.starts_with("GET /redirect ") {
        let response = "HTTP/1.1 302 Found\r\nLocation: /page\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(response.as_bytes());
        return;
    }
    if request_line.starts_with("GET /download ") {
        let body = b"Firefox native download fixture";
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nContent-Disposition: attachment; filename=fixture.txt\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(body);
        return;
    }
    let body = br#"<!doctype html><title>Firefox fixture</title>
<input id="name"><button id="go" onclick="document.title='clicked'">go</button><a id="download" href="/download">download</a>
<script>localStorage.setItem('native','local');sessionStorage.setItem('native','session');</script>"#;
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}

#[test]
fn firefox_fixture_handles_a_fragmented_redirect_request_line() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind Firefox fixture test");
    listener
        .set_nonblocking(true)
        .expect("make Firefox fixture test listener nonblocking");
    let endpoint = listener.local_addr().expect("fixture test address");
    let serving = thread::spawn(move || {
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept fixture test request: {error}"),
            }
        };
        serve_fixture_request(&mut stream);
    });

    let mut stream = TcpStream::connect(endpoint).expect("connect Firefox fixture test");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set fixture test read timeout");
    stream
        .write_all(b"GET /redirect ")
        .expect("write partial redirect request line");
    thread::sleep(Duration::from_millis(50));
    stream
        .write_all(b"HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .expect("finish redirect request line");

    let mut response = String::new();
    std::io::Read::read_to_string(&mut stream, &mut response)
        .expect("read fixture redirect response");
    serving.join().expect("join Firefox fixture test server");
    assert!(
        response.starts_with("HTTP/1.1 302 Found\r\n"),
        "fragmented redirect request received unexpected response: {response:?}"
    );
    assert!(response.contains("Location: /page\r\n"));
}

async fn evaluate_string(session: &mut FirefoxSession, expression: &str) -> String {
    session
        .evaluate(expression)
        .await
        .expect("evaluate Firefox fixture")
        .value
        .and_then(|value| value.as_str().map(str::to_owned))
        .expect("Firefox expression returned a string")
}

async fn wait_for_string(session: &mut FirefoxSession, expression: &str, expected: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let actual = evaluate_string(session, expression).await;
        if actual == expected {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Firefox value did not reach {expected:?}; last value: {actual:?}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_download(path: &std::path::Path, expected: &[u8]) -> Vec<u8> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut observed = None;
    loop {
        if let Ok(bytes) = std::fs::read(path) {
            if bytes == expected {
                return bytes;
            }
            observed = Some(bytes);
        }
        let now = tokio::time::Instant::now();
        assert!(
            now < deadline,
            "Firefox download at {} did not reach the expected bytes before deadline; last observed: {:?}",
            path.display(),
            observed
        );
        tokio::time::sleep(Duration::from_millis(50).min(deadline - now)).await;
    }
}

async fn wait_for_endpoint_closed(endpoint: std::net::SocketAddr) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if TcpStream::connect_timeout(&endpoint, Duration::from_millis(100)).is_err() {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Firefox BiDi endpoint remained open after process-tree cleanup"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
#[ignore = "native gate: set SYMBROWSE_E2E=1 and pass -- --ignored"]
async fn native_firefox_bidi_fixture_checks_supported_capabilities() {
    assert_eq!(
        std::env::var("SYMBROWSE_E2E").as_deref(),
        Ok("1"),
        "set SYMBROWSE_E2E=1 to execute the native Firefox gate"
    );
    let executable = resolve_firefox_executable(
        std::env::var_os("SYMBROWSE_FIREFOX_EXECUTABLE")
            .as_deref()
            .map(PathBuf::from)
            .as_deref(),
    )
    .expect("native Firefox must be installed or explicitly configured");
    let capabilities = canonical_capabilities();
    for interface in [
        "CookieEngine",
        "FrameManager",
        "InspectionEngine",
        "InteractionEngine",
        "NavigationStateProvider",
        "ScreenshotEngine",
        "TabManager",
    ] {
        assert!(
            capabilities.interfaces.iter().any(|item| item == interface),
            "Firefox capability missing: {interface}"
        );
    }
    for unsupported in ["downloads", "network.capture"] {
        assert!(matches!(
            FirefoxSession::unsupported(unsupported),
            FirefoxError::Unsupported { .. }
        ));
    }

    let temp = tempfile::tempdir().expect("create owned Firefox test directory");
    let profile = temp.path().join("profile");
    let download_dir = temp.path().join("downloads");
    std::fs::create_dir(&profile).expect("create owned Firefox profile");
    std::fs::write(
        profile.join("user.js"),
        b"user_pref(\"remote.experimental.enabled\", true);\n",
    )
    .expect("enable Nightly-only BiDi network and download features in isolated profile");
    std::fs::create_dir(&download_dir).expect("create owned download directory");
    let fixture = FixtureServer::start();
    let mut session = FirefoxSession::launch(executable.clone(), profile, Duration::from_secs(20))
        .await
        .expect("launch Firefox with the owned isolated profile");
    session
        .start_response_capture()
        .await
        .expect("subscribe to Firefox network response events");

    session
        .navigate(&format!("{}/redirect", fixture.base_url()))
        .await
        .expect("follow fixture redirect");
    wait_for_string(
        &mut session,
        "location.href",
        &format!("{}/page", fixture.base_url()),
    )
    .await;
    assert_eq!(
        evaluate_string(&mut session, "document.title").await,
        "Firefox fixture"
    );
    let responses = session
        .take_response_capture()
        .await
        .expect("drain Firefox response events");
    assert!(
        responses.iter().any(|event| {
            event["method"] == "network.responseCompleted"
                && event["params"]["request"]["url"] == format!("{}/page", fixture.base_url())
                && event["params"]["response"]["status"] == 200
        }),
        "Firefox response capture omitted the fixture response: {responses:?}"
    );

    let stored = evaluate_string(
        &mut session,
        "JSON.stringify([localStorage.getItem('native'),sessionStorage.getItem('native')])",
    )
    .await;
    assert_eq!(stored, r#"["local","session"]"#);
    let cookie = json!({
        "name": "native",
        "value": {"type": "string", "value": "cookie"},
        "domain": "127.0.0.1",
        "path": "/",
        "secure": false,
        "httpOnly": false,
        "sameSite": "lax"
    });
    session
        .set_cookie(cookie)
        .await
        .expect("set cookie through Firefox BiDi");
    let cookies = session.cookies().await.expect("read cookies through BiDi");
    assert!(
        cookies["cookies"].as_array().is_some_and(|items| {
            items
                .iter()
                .any(|item| item["cookie"]["name"] == "native" || item["name"] == "native")
        }),
        "Firefox cookie result did not contain the fixture cookie: {cookies}"
    );

    session
        .interact("fill", "#name", Some("native"))
        .await
        .expect("fill fixture input");
    assert_eq!(
        evaluate_string(&mut session, "document.querySelector('#name').value").await,
        "native"
    );
    session
        .interact("click", "#go", None)
        .await
        .expect("click fixture button");
    assert_eq!(
        evaluate_string(&mut session, "document.title").await,
        "clicked"
    );
    let contexts = session
        .browsing_contexts()
        .await
        .expect("list Firefox browsing contexts");
    assert!(
        contexts["contexts"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
    let screenshot = session
        .screenshot("png")
        .await
        .expect("capture Firefox viewport");
    assert!(
        screenshot["data"]
            .as_str()
            .is_some_and(|data| !data.is_empty())
    );

    session
        .allow_downloads(&download_dir)
        .await
        .expect("configure downloads inside the owned isolated directory");
    session
        .interact("click", "#download", None)
        .await
        .expect("start the fixture download");
    assert_eq!(
        wait_for_download(
            &download_dir.join("fixture.txt"),
            b"Firefox native download fixture",
        )
        .await,
        b"Firefox native download fixture"
    );

    session.set_timeout(Duration::from_secs(1));
    let timeout = session
        .evaluate("new Promise(() => {})")
        .await
        .expect_err("unresolved JavaScript promise must hit the BiDi deadline");
    assert!(
        matches!(timeout, FirefoxError::Timeout { ref operation, timeout } if operation == "script.evaluate" && timeout == Duration::from_secs(1)),
        "unexpected Firefox timeout error: {timeout}"
    );

    let endpoint = session.remote_endpoint();
    session.close().await.expect("close owned Firefox process");
    wait_for_endpoint_closed(endpoint).await;
    let fixture_url = fixture.base_url();
    tokio::task::spawn_blocking(move || {
        production_daemon_uses_the_selected_firefox(executable, &fixture_url)
    })
    .await
    .expect("join Firefox production-daemon fixture");
    drop(fixture);
    drop(temp);
}
