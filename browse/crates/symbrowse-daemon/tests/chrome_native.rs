use std::{
    fs,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
use symbrowse_daemon::{Client, ClientOptions, Frame, Server, ServerOptions, SessionSpec};

fn enabled() -> bool {
    std::env::var_os("SYMBROWSE_E2E").as_deref() == Some(std::ffi::OsStr::new("1"))
}

fn request(client: &Client, command: &str, args: Value) -> Value {
    let diagnostic_args = args.clone();
    let started = Instant::now();
    let frame = Frame {
        cmd: command.to_owned(),
        args: Some(args),
        session: client.options().session.clone(),
        ..Frame::default()
    };
    let response = client.request(frame).unwrap_or_else(|error| {
        panic!(
            "request {command} args={diagnostic_args}: daemon request failed after {:?}: {error}",
            started.elapsed()
        )
    });
    if started.elapsed() >= Duration::from_secs(2) {
        eprintln!(
            "chrome_native_slow_command={command} elapsed={:?}",
            started.elapsed()
        );
    }
    serde_json::to_value(response).expect("encode response")
}

struct ChromeContractServer {
    base_url: String,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl ChromeContractServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind Chrome contract fixture");
        let address = listener
            .local_addr()
            .expect("Chrome contract fixture address");
        listener
            .set_nonblocking(true)
            .expect("set Chrome contract fixture nonblocking");
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !stop_for_thread.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .expect("bound Chrome contract fixture read");
                        let mut request = [0_u8; 2048];
                        let length = stream.read(&mut request).unwrap_or_default();
                        let request = String::from_utf8_lossy(&request[..length]);
                        let path = request
                            .lines()
                            .next()
                            .and_then(|line| line.split_whitespace().nth(1))
                            .unwrap_or("/");
                        let (content_type, extra_headers, body) = if path == "/download" {
                            (
                                "application/octet-stream",
                                "Content-Disposition: attachment; filename=\"fixture.txt\"\r\n",
                                "symbrowse native download fixture\n",
                            )
                        } else if path == "/popup" {
                            (
                                "text/html; charset=utf-8",
                                "",
                                "<!doctype html><button id=popup title='popup opener' onclick=\"window.popup=window.open('/popup-child','symbrowse-popup')\">Open popup</button><a id=link href='/destination'>Destination</a><div id=plain>Plain element</div><div id=hidden>fallback text</div><button id=covered style='position:fixed;left:10px;top:100px;width:160px;height:50px'>Covered</button><div id=cover aria-label='cover' style='position:fixed;z-index:2;left:10px;top:100px;width:160px;height:50px'></div><script>Object.defineProperty(document.querySelector('#hidden'),'innerText',{get(){return ''}})</script>",
                            )
                        } else {
                            (
                                "text/html; charset=utf-8",
                                "",
                                "<!doctype html><title>Chrome contract</title><p>managed tab fixture</p><a id=download href='/download'>download</a><div style='height:12000px'><div id=target tabindex=0 style='margin-top:8000px;height:100px'></div></div>",
                            )
                        };
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len(),
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            base_url: format!("http://{address}"),
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for ChromeContractServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn go_chrome_tab_oracle(executable: &str, fixture_url: &str) -> Value {
    let browse_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("go")
        .args([
            "run",
            "./port/harness/cmd/chrome-contract-oracle",
            executable,
            fixture_url,
        ])
        .current_dir(browse_root)
        .env("SYMBROWSE_HEADLESS", "1")
        .output()
        .expect("run source-bound Go Chrome tab oracle");
    assert!(
        output.status.success(),
        "Go Chrome tab oracle failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("decode Go Chrome tab oracle JSON")
}

fn normalize_captured_request(mut request: Value) -> Value {
    if let Some(object) = request.as_object_mut() {
        object.remove("id");
        object.remove("started_at");
    }
    request
}

fn normalize_runtime_event_response(mut data: Value) -> Value {
    if let Some(entries) = data.get_mut("entries").and_then(Value::as_array_mut) {
        for entry in entries {
            if let Some(entry) = entry.as_object_mut() {
                entry.insert("timestamp".into(), Value::String("<timestamp>".into()));
            }
        }
    }
    data
}

fn wait_for_runtime_entries(client: &Client, command: &str, count: usize) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let response = request(client, command, json!({}));
        assert_eq!(response["success"], true, "{command}: {response}");
        if response["data"]["entries"]
            .as_array()
            .is_some_and(|entries| entries.len() >= count)
        {
            return response;
        }
        assert!(
            Instant::now() < deadline,
            "{command} did not capture {count} entries: {response}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn accept_fixture_request(
    listener: &TcpListener,
    timeout: Duration,
) -> io::Result<(TcpStream, std::net::SocketAddr)> {
    let deadline = Instant::now() + timeout;
    loop {
        match listener.accept() {
            Ok(accepted) => return Ok(accepted),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "timed out waiting for Chrome's network fixture request",
                    ));
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(error),
        }
    }
}

#[test]
fn network_fixture_accept_has_a_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture listener");
    listener
        .set_nonblocking(true)
        .expect("set fixture listener nonblocking");
    let error = accept_fixture_request(&listener, Duration::from_millis(50))
        .expect_err("no client should connect");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(
        error
            .to_string()
            .contains("Chrome's network fixture request")
    );
}

#[test]
fn production_daemon_path_runs_chrome_over_platform_transport() {
    if !enabled() {
        return;
    }
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "symbrowse-daemon-chrome-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("create isolated root");
    let session = format!("native-chrome-{}", std::process::id());
    let mut spec = SessionSpec::for_session(&session);
    spec.state_dir = root.join("state");
    spec.cache_dir = root.join("cache");
    spec.daemon_log = root.join("daemon.log");
    let mut private_socket_dir = None;
    if cfg!(windows) {
        spec.socket_path = symbrowse_daemon::default_socket_path(&session);
    } else if cfg!(target_os = "macos") {
        // Keep the sockaddr path short without asking the daemon to chmod the
        // shared /tmp directory. The daemon secures only this private child.
        let directory =
            PathBuf::from("/private/tmp").join(format!("sbchrome-{}", std::process::id()));
        fs::create_dir(&directory).expect("create private socket directory");
        spec.socket_path = directory.join("daemon.sock");
        private_socket_dir = Some(directory);
    } else {
        spec.socket_path = root.join("daemon.sock");
    }
    spec.operation_timeout = Duration::from_secs(45);
    spec.idle_timeout = Some(Duration::from_secs(30));
    let profile = spec.user_data_dir();
    let expect_unavailable = std::env::var_os("SYMBROWSE_E2E_EXPECT_CHROME_UNAVAILABLE")
        .is_some_and(|value| value == "1");
    let chrome_version = std::env::var("SYMBROWSE_CHROME_VERSION").ok();
    if expect_unavailable {
        assert!(
            std::env::var_os("SYMBROWSE_CHROME_EXECUTABLE").is_some(),
            "unavailable target must use an explicit missing Chrome path"
        );
        assert!(
            chrome_version.is_none(),
            "unavailable target has no Chrome version"
        );
    } else {
        assert!(
            std::env::var_os("SYMBROWSE_CHROME_EXECUTABLE").is_some(),
            "set SYMBROWSE_CHROME_EXECUTABLE to the isolated Chrome for Testing binary"
        );
        assert!(
            chrome_version
                .as_deref()
                .is_some_and(|version| !version.is_empty()),
            "set SYMBROWSE_CHROME_VERSION from the tested executable"
        );
        eprintln!(
            "native_e2e_engine=chrome version={}",
            chrome_version.unwrap()
        );
    }

    // The Go oracle launches and exercises its own browser. Run it before the
    // Rust daemon starts so slow Windows startup cannot exceed the daemon's
    // 30-second idle timeout before the first Rust request.
    let (contract_server, go_oracle) = if expect_unavailable {
        (None, None)
    } else {
        let chrome_executable = std::env::var("SYMBROWSE_CHROME_EXECUTABLE")
            .expect("set SYMBROWSE_CHROME_EXECUTABLE to the tested CfT binary");
        let contract_server = ChromeContractServer::start();
        let go_oracle = go_chrome_tab_oracle(&chrome_executable, &contract_server.base_url);
        (Some(contract_server), Some(go_oracle))
    };

    let server = std::sync::Arc::new(
        Server::new(ServerOptions {
            session_spec: Some(spec.clone()),
            operation_timeout: Duration::from_secs(45),
            idle_timeout: Some(Duration::from_secs(30)),
            ..ServerOptions::default()
        })
        .expect("create daemon"),
    );
    let socket = spec.socket_path.clone();
    let server_for_thread = std::sync::Arc::clone(&server);
    let thread = thread::spawn(move || server_for_thread.listen_and_serve().expect("serve daemon"));
    let client = Client::new(ClientOptions {
        socket_path: socket.clone(),
        session: session.clone(),
        read_timeout: Duration::from_secs(60),
        startup_timeout: Duration::from_secs(5),
        autostart: false,
        expected_engine: Some("chrome".into()),
        ..ClientOptions::default()
    });
    let deadline = Instant::now() + Duration::from_secs(10);
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
            Instant::now() < deadline,
            "daemon endpoint did not become ready"
        );
        thread::sleep(Duration::from_millis(20));
    }

    let capabilities = request(&client, "capabilities", json!({}));
    assert_eq!(capabilities["success"], true);
    if expect_unavailable {
        let opened = request(
            &client,
            "open",
            json!({"url": "data:text/html,<title>must-not-fallback</title>"}),
        );
        assert_eq!(opened["success"], false, "open response: {opened}");
        assert!(
            matches!(
                opened["error"]["code"].as_str(),
                Some("unavailable" | "precondition" | "daemon_unavailable")
            ),
            "missing Chrome must return the typed unavailable/precondition error: {opened}"
        );
        assert!(
            opened["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("SYMBROWSE_CHROME_EXECUTABLE")),
            "error must identify the explicit missing executable: {opened}"
        );
        server.stop();
        thread.join().expect("join daemon");
        assert!(
            profile
                .read_dir()
                .is_ok_and(|mut entries| entries.next().is_none()),
            "an unavailable Chrome launch must not write browser profile data"
        );
        if let Some(directory) = private_socket_dir {
            fs::remove_dir_all(directory).expect("remove private socket directory");
        }
        fs::remove_dir_all(root).expect("remove isolated root");
        return;
    }
    let contract_server = contract_server.expect("create Chrome contract fixture");
    let go_oracle = go_oracle.expect("run Go Chrome oracle");
    let rust_open = request(
        &client,
        "open",
        json!({"url":format!("{}/page", contract_server.base_url)}),
    );
    assert_eq!(
        rust_open["success"], go_oracle["open"]["success"],
        "open response: Rust={rust_open} Go={}",
        go_oracle["open"]
    );
    assert_eq!(
        rust_open["data"], go_oracle["open"]["data"],
        "open result fields: Rust={rust_open} Go={}",
        go_oracle["open"]
    );
    let rust_open_fragment = request(
        &client,
        "open",
        json!({"url":format!("{}/page#same-document", contract_server.base_url)}),
    );
    assert_eq!(
        rust_open_fragment["success"], go_oracle["open_fragment"]["success"],
        "same-document open response: Rust={rust_open_fragment} Go={}",
        go_oracle["open_fragment"]
    );
    assert_eq!(
        rust_open_fragment["data"]["url"], go_oracle["open_fragment"]["data"]["url"],
        "same-document open URL: Rust={rust_open_fragment} Go={}",
        go_oracle["open_fragment"]
    );
    assert_eq!(
        rust_open_fragment["data"], go_oracle["open_fragment"]["data"],
        "same-document open fields: Rust={rust_open_fragment} Go={}",
        go_oracle["open_fragment"]
    );
    let rust_open_relative = request(&client, "open", json!({"url":"relative-probe"}));
    assert_eq!(
        rust_open_relative["success"], go_oracle["open_relative"]["success"],
        "relative open response: Rust={rust_open_relative} Go={}",
        go_oracle["open_relative"]
    );
    assert_eq!(
        rust_open_relative["data"]["url"], go_oracle["open_relative"]["data"]["url"],
        "relative open URL: Rust={rust_open_relative} Go={}",
        go_oracle["open_relative"]
    );
    assert_eq!(
        rust_open_relative["data"], go_oracle["open_relative"]["data"],
        "relative open fields: Rust={rust_open_relative} Go={}",
        go_oracle["open_relative"]
    );
    let network_started = request(&client, "network.requests", json!({}));
    assert_eq!(
        network_started["success"], go_oracle["network_capture"]["success"],
        "network.requests capture start: rust={network_started}, go={}",
        go_oracle["network_capture"]
    );
    let rust_fixture_scroll = request(&client, "scrollintoview", json!({"selector":"#target"}));
    assert_eq!(
        rust_fixture_scroll["success"], go_oracle["scroll_into_view"]["success"],
        "scrollintoview success: Rust={rust_fixture_scroll} Go={}",
        go_oracle["scroll_into_view"]
    );
    assert_eq!(
        rust_fixture_scroll["data"], go_oracle["scroll_into_view"]["data"],
        "scrollintoview result: Rust={rust_fixture_scroll} Go={}",
        go_oracle["scroll_into_view"]
    );
    let reloaded = request(&client, "reload", json!({}));
    assert_eq!(
        reloaded["success"], true,
        "reload after capture start: {reloaded}"
    );
    let download_dir = root.join("downloads");
    let set_download_dir = request(
        &client,
        "download.setdir",
        json!({"dir":download_dir.to_string_lossy()}),
    );
    assert_eq!(
        set_download_dir["success"], true,
        "set download dir: {set_download_dir}"
    );
    assert_eq!(
        set_download_dir["data"]["download_dir"],
        download_dir.to_string_lossy().as_ref()
    );
    let download_click = request(&client, "click", json!({"selector":"#download"}));
    assert_eq!(
        download_click["success"], true,
        "download click: {download_click}"
    );
    let downloads_deadline = Instant::now() + Duration::from_secs(10);
    let downloads = loop {
        let listed = request(&client, "downloads.list", json!({}));
        assert_eq!(listed["success"], true, "downloads.list: {listed}");
        let events = listed["data"]["downloads"].as_array();
        if events
            .and_then(|events| events.last())
            .is_some_and(|event| {
                event["state"] == "completed"
                    && event["sha256"]
                        == "4a47af0e5370e54cdf18a00ec82c4b235927baee89437ffa6a67ae7e85c5290c"
            })
        {
            break listed["data"]["downloads"].clone();
        }
        assert!(
            Instant::now() < downloads_deadline,
            "download did not complete with a GUID-file checksum: {listed}"
        );
        thread::sleep(Duration::from_millis(25));
    };
    assert_eq!(downloads.as_array().map(Vec::len), Some(1));
    assert_eq!(downloads[0]["filename"], "fixture.txt");
    assert_eq!(
        downloads[0]["url"],
        format!("{}/download", contract_server.base_url)
    );
    assert_eq!(downloads[0]["received_bytes"], 34);
    assert_eq!(downloads[0]["total_bytes"], 34);
    let rust_network_requests = request(&client, "network.requests", json!({}));
    assert_eq!(
        rust_network_requests["success"], go_oracle["network_requests"]["success"],
        "network.requests result: rust={rust_network_requests}, go={}",
        go_oracle["network_requests"]
    );
    let page_url = format!("{}/page", contract_server.base_url);
    let rust_page_request = rust_network_requests["data"]["requests"]
        .as_array()
        .and_then(|requests| requests.iter().find(|request| request["url"] == page_url))
        .cloned()
        .expect("Rust network.requests captured page request");
    let go_page_request = go_oracle["network_requests"]["data"]["requests"]
        .as_array()
        .and_then(|requests| requests.iter().find(|request| request["url"] == page_url))
        .cloned()
        .expect("Go network.requests captured page request");
    assert_eq!(
        normalize_captured_request(rust_page_request.clone()),
        normalize_captured_request(go_page_request.clone()),
        "network.requests page record"
    );
    let rust_network_request = request(
        &client,
        "network.request",
        json!({"id":rust_page_request["id"]}),
    );
    assert_eq!(
        rust_network_request["success"], go_oracle["network_request"]["success"],
        "network.request result: rust={rust_network_request}, go={}",
        go_oracle["network_request"]
    );
    assert_eq!(
        normalize_captured_request(rust_network_request["data"]["request"].clone()),
        normalize_captured_request(go_oracle["network_request"]["data"]["request"].clone()),
        "network.request record"
    );
    let rust_missing_request = request(
        &client,
        "network.request",
        json!({"id":"missing-network-request"}),
    );
    assert_eq!(
        rust_missing_request["success"], go_oracle["network_missing_request"]["success"],
        "missing network.request result: rust={rust_missing_request}, go={}",
        go_oracle["network_missing_request"]
    );
    assert_eq!(
        rust_missing_request["error"]["code"],
        go_oracle["network_missing_request"]["error"]["code"]
    );
    assert_eq!(
        rust_missing_request["error"]["message"],
        go_oracle["network_missing_request"]["error"]["message"]
    );
    let rust_console_initial = request(&client, "console.list", json!({}));
    assert_eq!(
        rust_console_initial["success"],
        go_oracle["runtime_console_initial"]["success"]
    );
    assert_eq!(
        normalize_runtime_event_response(rust_console_initial["data"].clone()),
        normalize_runtime_event_response(go_oracle["runtime_console_initial"]["data"].clone()),
        "initial console.list"
    );
    let rust_console_emit = request(
        &client,
        "eval",
        json!({"expression":"console.warn('symbrowse runtime console probe')"}),
    );
    assert_eq!(
        rust_console_emit["success"], go_oracle["runtime_console_emit"]["success"],
        "console probe eval: {rust_console_emit}"
    );
    let rust_console_list = wait_for_runtime_entries(&client, "console.list", 1);
    assert_eq!(
        normalize_runtime_event_response(rust_console_list["data"].clone()),
        normalize_runtime_event_response(go_oracle["runtime_console_list"]["data"].clone()),
        "console.list emitted entry"
    );
    let rust_console_clear = request(&client, "console.clear", json!({}));
    assert_eq!(
        rust_console_clear["success"],
        go_oracle["runtime_console_clear"]["success"]
    );
    assert_eq!(
        rust_console_clear["data"],
        go_oracle["runtime_console_clear"]["data"]
    );
    let rust_console_cleared = request(&client, "console.list", json!({}));
    assert_eq!(
        normalize_runtime_event_response(rust_console_cleared["data"].clone()),
        normalize_runtime_event_response(go_oracle["runtime_console_cleared"]["data"].clone()),
        "console.list after clear"
    );
    let rust_errors_initial = request(&client, "errors.list", json!({}));
    assert_eq!(
        rust_errors_initial["success"],
        go_oracle["runtime_errors_initial"]["success"]
    );
    assert_eq!(
        normalize_runtime_event_response(rust_errors_initial["data"].clone()),
        normalize_runtime_event_response(go_oracle["runtime_errors_initial"]["data"].clone()),
        "initial errors.list"
    );
    let rust_exception_emit = request(
        &client,
        "eval",
        json!({"expression":"setTimeout(() => { throw new Error('symbrowse uncaught runtime probe') }, 0)"}),
    );
    assert_eq!(
        rust_exception_emit["success"], go_oracle["runtime_exception_emit"]["success"],
        "uncaught exception eval: {rust_exception_emit}"
    );
    let rust_errors_list = wait_for_runtime_entries(&client, "errors.list", 1);
    assert_eq!(
        normalize_runtime_event_response(rust_errors_list["data"].clone()),
        normalize_runtime_event_response(go_oracle["runtime_errors_list"]["data"].clone()),
        "errors.list emitted entry"
    );
    let rust_errors_clear = request(&client, "errors.clear", json!({}));
    assert_eq!(
        rust_errors_clear["success"],
        go_oracle["runtime_errors_clear"]["success"]
    );
    assert_eq!(
        rust_errors_clear["data"],
        go_oracle["runtime_errors_clear"]["data"]
    );
    let rust_errors_cleared = request(&client, "errors.list", json!({}));
    assert_eq!(
        normalize_runtime_event_response(rust_errors_cleared["data"].clone()),
        normalize_runtime_event_response(go_oracle["runtime_errors_cleared"]["data"].clone()),
        "errors.list after clear"
    );
    let rust_tab_new = request(
        &client,
        "tab.new",
        json!({
            "label":"second",
            "url":format!("{}/popup", contract_server.base_url)
        }),
    );
    assert_eq!(rust_tab_new["success"], go_oracle["tab_new"]["success"]);
    assert_eq!(rust_tab_new["data"], go_oracle["tab_new"]["data"]);
    for (command, oracle_key, args) in [
        ("get.text", "inspect_text", json!({"selector":"#popup"})),
        ("get.html", "inspect_html", json!({})),
        ("get.title", "inspect_title", json!({})),
        ("get.url", "inspect_url", json!({})),
        (
            "get.title",
            "inspect_selected_title",
            json!({"selector":"#popup"}),
        ),
        (
            "get.url",
            "inspect_selected_url",
            json!({"selector":"#link"}),
        ),
        ("get.value", "inspect_value", json!({"selector":"#plain"})),
        (
            "get.text",
            "inspect_hidden_text",
            json!({"selector":"#hidden"}),
        ),
        ("get.box", "inspect_box", json!({"selector":"#popup"})),
        ("get.styles", "inspect_styles", json!({"selector":"#popup"})),
        (
            "get.styles",
            "inspect_styles_wanted",
            json!({"selector":"#popup","properties":["display","visibility","color"]}),
        ),
        (
            "is.visible",
            "inspect_visible",
            json!({"selector":"#popup"}),
        ),
    ] {
        let inspected = request(&client, command, args);
        assert_eq!(
            inspected["success"], go_oracle[oracle_key]["success"],
            "{command} success differs: Rust={inspected} Go={}",
            go_oracle[oracle_key]
        );
        assert_eq!(
            inspected["data"], go_oracle[oracle_key]["data"],
            "{command} data differs"
        );
    }
    let rust_inspect_error = request(&client, "get.text", json!({}));
    assert_eq!(
        rust_inspect_error["success"],
        go_oracle["inspect_error"]["success"]
    );
    assert_eq!(
        rust_inspect_error["error"]["code"],
        go_oracle["inspect_error"]["error"]["code"]
    );
    assert_eq!(
        rust_inspect_error["error"]["message"],
        go_oracle["inspect_error"]["error"]["message"]
    );
    let rust_missing_element = request(&client, "get.title", json!({"selector":"#missing"}));
    assert_eq!(
        rust_missing_element["success"],
        go_oracle["inspect_missing_element"]["success"]
    );
    assert_eq!(
        rust_missing_element["error"]["code"],
        go_oracle["inspect_missing_element"]["error"]["code"]
    );
    assert_eq!(
        rust_missing_element["error"]["message"],
        go_oracle["inspect_missing_element"]["error"]["message"]
    );
    let rust_click_obstructed = request(&client, "click", json!({"selector":"#covered"}));
    assert_eq!(
        rust_click_obstructed["success"], go_oracle["click_obstructed"]["success"],
        "click obstruction success differs: Rust={rust_click_obstructed} Go={}",
        go_oracle["click_obstructed"]
    );
    assert_eq!(
        rust_click_obstructed["error"]["code"],
        go_oracle["click_obstructed"]["error"]["code"]
    );
    assert_eq!(
        rust_click_obstructed["error"]["message"],
        go_oracle["click_obstructed"]["error"]["message"]
    );
    assert_eq!(
        rust_click_obstructed["error"]["hint"],
        go_oracle["click_obstructed"]["error"]["hint"]
    );
    let rust_popup_click = request(&client, "click", json!({"selector":"#popup"}));
    assert_eq!(
        rust_popup_click["success"],
        go_oracle["popup_click"]["success"]
    );
    assert_eq!(rust_popup_click["data"], go_oracle["popup_click"]["data"]);
    let rust_popup_open = request(
        &client,
        "eval",
        json!({"expression":"Boolean(window.popup && !window.popup.closed)"}),
    );
    assert_eq!(
        rust_popup_open["success"], go_oracle["popup_open"]["success"],
        "popup evaluation differs: Rust={rust_popup_open} Go={}",
        go_oracle["popup_open"]
    );
    assert_eq!(
        rust_popup_open["data"]["type"],
        go_oracle["popup_open"]["data"]["type"]
    );
    assert_eq!(
        rust_popup_open["data"]["value"],
        go_oracle["popup_open"]["data"]["value"]
    );
    let rust_eval_exception = request(
        &client,
        "eval",
        json!({"expression":"(() => { throw new Error('native eval failure') })()"}),
    );
    assert_eq!(
        rust_eval_exception["success"], go_oracle["eval_exception"]["success"],
        "eval exception success differs: Rust={rust_eval_exception} Go={}",
        go_oracle["eval_exception"]
    );
    assert_eq!(
        rust_eval_exception["data"], go_oracle["eval_exception"]["data"],
        "eval exception result differs"
    );
    let rust_tab_list = request(&client, "tab.list", json!({}));
    assert_eq!(rust_tab_list["success"], go_oracle["tab_list"]["success"]);
    assert_eq!(rust_tab_list["data"], go_oracle["tab_list"]["data"]);
    let rust_tab_close = request(&client, "tab.close", json!({"tab":"second"}));
    assert_eq!(rust_tab_close["success"], go_oracle["tab_close"]["success"]);
    assert_eq!(rust_tab_close["data"], go_oracle["tab_close"]["data"]);
    let rust_last_tab_close = request(&client, "tab.close", json!({}));
    assert_eq!(
        rust_last_tab_close["success"],
        go_oracle["last_tab_close"]["success"]
    );
    assert_eq!(
        rust_last_tab_close["error"]["code"],
        go_oracle["last_tab_close"]["error"]["code"]
    );
    assert_eq!(
        rust_last_tab_close["error"]["message"],
        go_oracle["last_tab_close"]["error"]["message"]
    );
    let rust_open_blank = request(&client, "open", json!({"url":"about:blank"}));
    assert_eq!(
        rust_open_blank["success"], go_oracle["open_blank"]["success"],
        "about:blank open response: Rust={rust_open_blank} Go={}",
        go_oracle["open_blank"]
    );
    assert_eq!(
        rust_open_blank["data"]["url"], go_oracle["open_blank"]["data"]["url"],
        "about:blank open URL: Rust={rust_open_blank} Go={}",
        go_oracle["open_blank"]
    );
    assert_eq!(
        rust_open_blank["data"], go_oracle["open_blank"]["data"],
        "about:blank open fields: Rust={rust_open_blank} Go={}",
        go_oracle["open_blank"]
    );
    let opened = request(
        &client,
        "open",
        json!({"url": "data:text/html,<title>daemon</title><h1>native</h1><div style='height:12000px'><div id='target' style='margin-top:8000px;height:100px'></div></div>"}),
    );
    assert_eq!(
        opened["success"], go_oracle["open_data"]["success"],
        "data: open response: Rust={opened} Go={}",
        go_oracle["open_data"]
    );
    assert_eq!(
        opened["data"]["url"], go_oracle["open_data"]["data"]["url"],
        "data: open URL: Rust={opened} Go={}",
        go_oracle["open_data"]
    );
    assert_eq!(
        opened["data"], go_oracle["open_data"]["data"],
        "data: open fields: Rust={opened} Go={}",
        go_oracle["open_data"]
    );
    let script = request(&client, "read", json!({}));
    assert!(
        script["data"]
            .as_str()
            .is_some_and(|text| text.contains("native")),
        "read response: {script}"
    );
    let popup_source = request(
        &client,
        "tab.new",
        json!({
            "label": "popup-source",
            "url": "data:text/html,%3Cbutton%20id%3Dpopup%20onclick%3D%22window.popup%3Dwindow.open%28%27about%3Ablank%27%2C%27symbrowse-popup%27%29%22%3EOpen%20popup%3C%2Fbutton%3E"
        }),
    );
    assert_eq!(
        popup_source["success"], true,
        "popup source: {popup_source}"
    );
    assert_eq!(popup_source["data"]["tab"], "t2");
    let popup_click = request(&client, "click", json!({"selector":"#popup"}));
    assert_eq!(popup_click["success"], true, "popup click: {popup_click}");
    let popup_open = request(
        &client,
        "eval",
        json!({"expression":"Boolean(window.popup && !window.popup.closed)"}),
    );
    assert_eq!(popup_open["success"], true, "popup eval: {popup_open}");
    assert_eq!(
        popup_open["data"]["value"], true,
        "popup eval: {popup_open}"
    );
    let popup_tabs = request(&client, "tab.list", json!({}));
    assert_eq!(popup_tabs["success"], true, "popup tab list: {popup_tabs}");
    assert_eq!(popup_tabs["data"]["tabs"].as_array().map(Vec::len), Some(2));
    assert_eq!(popup_tabs["data"]["active"], "t2");
    let popup_closed = request(&client, "tab.close", json!({"tab":"popup-source"}));
    assert_eq!(
        popup_closed["success"], true,
        "close popup source: {popup_closed}"
    );
    assert_eq!(popup_closed["data"]["active"], "t1");
    let into_view = request(&client, "scrollintoview", json!({"selector":"#target"}));
    assert_eq!(into_view["success"], true, "scroll into view: {into_view}");
    let scrolled_into_view = request(&client, "get.box", json!({"selector":"#target"}));
    assert_eq!(
        scrolled_into_view["success"], true,
        "pre-scroll box: {scrolled_into_view}"
    );
    let scrolled = request(&client, "scroll", json!({"selector":"#target","amount":0}));
    assert_eq!(
        scrolled["success"], true,
        "default scroll response: {scrolled}"
    );
    thread::sleep(Duration::from_millis(100));
    let scrolled_down = request(&client, "get.box", json!({"selector":"#target"}));
    assert_eq!(
        scrolled_down["success"], true,
        "post-scroll box: {scrolled_down}"
    );
    assert!(
        scrolled_down["data"]["value"]["y"]
            .as_f64()
            .unwrap_or_default()
            < scrolled_into_view["data"]["value"]["y"]
                .as_f64()
                .unwrap_or_default(),
        "positive scroll did not move the target up: before={scrolled_into_view}, after={scrolled_down}"
    );
    thread::sleep(Duration::from_millis(100));
    let scrolled_up = request(
        &client,
        "scroll",
        json!({"selector":"#target","amount":-240}),
    );
    assert_eq!(
        scrolled_up["success"], true,
        "negative scroll: {scrolled_up}"
    );
    let scrolled_up_box = request(&client, "get.box", json!({"selector":"#target"}));
    assert_eq!(
        scrolled_up_box["success"], true,
        "negative scroll box: {scrolled_up_box}"
    );
    assert!(
        scrolled_up_box["data"]["value"]["y"]
            .as_f64()
            .unwrap_or_default()
            > scrolled_down["data"]["value"]["y"]
                .as_f64()
                .unwrap_or_default(),
        "negative scroll did not move the target down: before={scrolled_down}, after={scrolled_up_box}"
    );
    let tabs = request(&client, "tabs.list", json!({}));
    assert_eq!(tabs["success"], true, "tabs response: {tabs}");
    let listed = tabs["data"]["tabs"].as_array().expect("tab list");
    assert!(!listed.is_empty(), "tabs response: {tabs}");
    let active = listed
        .iter()
        .filter(|tab| tab["active"] == true)
        .collect::<Vec<_>>();
    assert_eq!(active.len(), 1, "tabs response: {tabs}");
    assert_eq!(tabs["data"]["active"], active[0]["id"]);
    let created = request(
        &client,
        "tab.new",
        json!({"label":"second","url":"data:text/html,<h1>second</h1>"}),
    );
    assert_eq!(created["success"], true, "tab.new response: {created}");
    assert_eq!(created["data"]["tab"], "t2");
    assert_eq!(created["data"]["label"], "second");
    let listed_tabs = request(&client, "tab.list", json!({}));
    assert_eq!(
        listed_tabs["success"], true,
        "tab.list response: {listed_tabs}"
    );
    assert_eq!(
        listed_tabs["data"]["tabs"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(listed_tabs["data"]["active"], "t2");
    let switched = request(&client, "tab.switch", json!({"tab":"t1"}));
    assert_eq!(switched["success"], true, "tab.switch response: {switched}");
    let original = request(&client, "read", json!({}));
    assert!(
        original["data"]
            .as_str()
            .is_some_and(|text| text.contains("native")),
        "switched tab read: {original}"
    );
    let closed = request(&client, "tab.close", json!({"tab":"second"}));
    assert_eq!(closed["success"], true, "tab.close response: {closed}");
    assert_eq!(closed["data"]["closed"], "t2");
    assert_eq!(closed["data"]["active"], "t1");
    let remaining = request(&client, "tab.list", json!({}));
    assert_eq!(remaining["data"]["tabs"].as_array().map(Vec::len), Some(1));
    let window = request(&client, "window.new", json!({}));
    assert_eq!(window["success"], true, "window.new response: {window}");
    assert_eq!(window["data"]["tab"], "t2");
    let closed_window = request(&client, "tab.close", json!({}));
    assert_eq!(
        closed_window["success"], true,
        "close active tab: {closed_window}"
    );
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
    let address = listener.local_addr().expect("fixture address");
    let fixture = thread::spawn(move || {
        listener
            .set_nonblocking(true)
            .expect("set fixture listener nonblocking");
        let (mut stream, _) = accept_fixture_request(&listener, Duration::from_secs(15))?;
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let mut request = [0_u8; 1024];
        if stream.read(&mut request).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("read Chrome fixture request: {error}"),
            )
        })? == 0
        {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Chrome connected without sending an HTTP fixture request",
            ));
        }
        let body = b"<h1>network</h1>";
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(header.as_bytes()).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("write Chrome fixture header: {error}"),
            )
        })?;
        stream.write_all(body).map_err(|error| {
            io::Error::new(error.kind(), format!("write Chrome fixture body: {error}"))
        })?;
        Ok::<(), io::Error>(())
    });
    let started = request(&client, "network.capture", json!({}));
    assert_eq!(started["success"], true, "network capture start: {started}");
    assert_eq!(started["data"]["started"], true);
    let url = format!("http://{address}/");
    let network_page = request(&client, "open", json!({"url": url}));
    assert_eq!(
        network_page["success"], true,
        "network page: {network_page}"
    );
    fixture
        .join()
        .unwrap_or_else(|_| panic!("Chrome network fixture thread panicked"))
        .unwrap_or_else(|error| panic!("Chrome network fixture failed: {error}"));
    let captured = request(&client, "network.requests", json!({}));
    assert_eq!(captured["success"], true, "network capture: {captured}");
    assert!(
        captured["data"]["count"]
            .as_u64()
            .is_some_and(|count| count > 0)
    );
    assert!(
        captured["data"]["requests"]
            .as_array()
            .is_some_and(|events| events.iter().any(|event| event["status"] == 200)),
        "network capture: {captured}"
    );
    let cookie_url = format!("http://{address}/");
    let set_cookie = request(
        &client,
        "cookies.set",
        json!({
            "cookie":{"name":"fixture_sid","value":"fixture-cookie-value","domain":"","path":"/","secure":false,"http_only":false},
            "url":cookie_url
        }),
    );
    assert_eq!(
        set_cookie["success"], true,
        "cookie set failed: {set_cookie}"
    );
    let listed_cookies = request(&client, "cookies.list", json!({}));
    assert_eq!(listed_cookies["success"], true, "cookie list failed");
    assert_eq!(
        listed_cookies["data"]["origin"],
        format!("http://{address}")
    );
    assert!(
        listed_cookies["data"]["cookies"]
            .as_array()
            .is_some_and(|cookies| {
                cookies.iter().any(|cookie| {
                    cookie["name"] == "fixture_sid" && cookie["value"] == "fixture-cookie-value"
                })
            }),
        "cookie list omitted the isolated fixture cookie"
    );
    let cleared_cookie = request(
        &client,
        "cookies.clear",
        json!({"name":"fixture_sid","url":cookie_url}),
    );
    assert_eq!(cleared_cookie["success"], true, "cookie clear failed");
    let listed_after_clear = request(&client, "cookies.list", json!({}));
    assert_eq!(
        listed_after_clear["success"], true,
        "cookie relist failed: {listed_after_clear}"
    );
    assert!(
        !listed_after_clear["data"]["cookies"]
            .as_array()
            .is_some_and(|cookies| cookies.iter().any(|cookie| cookie["name"] == "fixture_sid")),
        "cleared fixture cookie remained visible"
    );
    let framed = request(
        &client,
        "open",
        json!({"url": "data:text/html,<iframe id='outer' srcdoc=\"<iframe id='inner' srcdoc='nested'></iframe>\"></iframe>"}),
    );
    assert_eq!(framed["success"], true, "frame page: {framed}");
    let frames = request(&client, "frame.tree", json!({}));
    assert_eq!(frames["success"], true, "frame response: {frames}");
    let roots = frames["data"]["frames"]
        .as_array()
        .expect("frame tree roots");
    assert_eq!(roots.len(), 1, "frame response: {frames}");
    let outer = roots[0]["children"][0].clone();
    assert_eq!(outer["name"], "outer", "frame response: {frames}");
    assert_eq!(
        outer["parent_id"], roots[0]["id"],
        "frame response: {frames}"
    );
    let inner = outer["children"][0].clone();
    assert_eq!(inner["name"], "inner", "frame response: {frames}");
    assert_eq!(inner["parent_id"], outer["id"], "frame response: {frames}");
    let flat = request(&client, "frames.list", json!({}));
    let flat_frames = flat["data"]["frames"].as_array().expect("flat frames");
    assert!(flat_frames.len() >= 2, "flat frame response: {flat}");
    assert!(
        flat_frames
            .iter()
            .all(|frame| frame.get("children").is_none())
    );
    let ax = request(&client, "a11y", json!({}));
    assert_eq!(ax["success"], true, "a11y response: {ax}");

    let interactions = request(
        &client,
        "open",
        json!({"url": "data:text/html,%3Cinput%20id%3D%27text%27%20onfocus%3D%22this.dataset.focused%3D%27yes%27%22%3E%3Cselect%20id%3D%27choice%27%3E%3Coption%20value%3D%27one%27%3EOne%3C%2Foption%3E%3Coption%20value%3D%27two%27%3ETwo%3C%2Foption%3E%3C%2Fselect%3E%3Cinput%20id%3D%27check%27%20type%3D%27checkbox%27%3E%3Cdiv%20id%3D%27dbl%27%20ondblclick%3D%22this.dataset.doubled%3D%27yes%27%22%3EDouble%3C%2Fdiv%3E%3Cdiv%20id%3D%27hover%27%20onmouseenter%3D%22this.dataset.hovered%3D%27yes%27%22%3EHover%3C%2Fdiv%3E"}),
    );
    assert_eq!(
        interactions["success"], true,
        "interaction page: {interactions}"
    );
    for (command, args) in [
        ("dblclick", json!({"selector":"#dbl"})),
        ("focus", json!({"selector":"#text"})),
        ("hover", json!({"selector":"#hover"})),
        ("select", json!({"selector":"#choice","value":"two"})),
        ("check", json!({"selector":"#check"})),
        ("uncheck", json!({"selector":"#check"})),
    ] {
        let response = request(&client, command, args);
        assert_eq!(response["success"], true, "{command} response: {response}");
        assert_eq!(response["data"]["action"], command);
        if command == "focus" {
            let focused = request(
                &client,
                "get.attr",
                json!({"selector":"#text","attribute":"data-focused"}),
            );
            assert_eq!(focused["data"], "yes", "focus state: {focused}");
        }
        if command == "check" {
            let checked = request(&client, "is.checked", json!({"selector":"#check"}));
            assert_eq!(checked["data"], true, "check state: {checked}");
        }
    }
    let doubled = request(
        &client,
        "get.attr",
        json!({"selector":"#dbl","attribute":"data-doubled"}),
    );
    assert_eq!(doubled["data"], "yes", "double-click state: {doubled}");
    let hovered = request(
        &client,
        "get.attr",
        json!({"selector":"#hover","attribute":"data-hovered"}),
    );
    assert_eq!(hovered["data"], "yes", "hover state: {hovered}");
    let selected = request(&client, "get.value", json!({"selector":"#choice"}));
    assert_eq!(selected["data"], "two", "select state: {selected}");
    let checked = request(&client, "is.checked", json!({"selector":"#check"}));
    assert_eq!(checked["data"], false, "uncheck state: {checked}");

    let no_dialog = request(&client, "dialog.status", json!({}));
    assert_eq!(
        no_dialog["success"], true,
        "empty dialog status: {no_dialog}"
    );
    assert_eq!(no_dialog["data"]["handled"], true);
    let prompt_page = request(
        &client,
        "open",
        json!({"url":"data:text/html,%3Cscript%3EsetTimeout(()%3D%3Eprompt('native%20prompt'%2C'seed')%2C100)%3C%2Fscript%3E"}),
    );
    assert_eq!(prompt_page["success"], true, "prompt page: {prompt_page}");
    thread::sleep(Duration::from_millis(250));
    let prompt_status = request(&client, "dialog.status", json!({}));
    assert_eq!(
        prompt_status["success"], true,
        "prompt status: {prompt_status}"
    );
    assert_eq!(prompt_status["data"]["type"], "prompt");
    assert_eq!(prompt_status["data"]["message"], "native prompt");
    assert_eq!(prompt_status["data"]["default"], "seed");
    assert_eq!(prompt_status["data"]["handled"], false);
    let accepted = request(&client, "dialog.accept", json!({"text":"answer"}));
    assert_eq!(accepted["success"], true, "dialog accept: {accepted}");
    assert_eq!(accepted["data"], json!({"handled":true,"action":"accept"}));
    let handled = request(&client, "dialog.status", json!({}));
    assert_eq!(
        handled["data"]["handled"], true,
        "handled status: {handled}"
    );

    let alert_page = request(
        &client,
        "open",
        json!({"url":"data:text/html,%3Cscript%3EsetTimeout(()%3D%3Ealert('native%20alert')%2C100)%3C%2Fscript%3E"}),
    );
    assert_eq!(alert_page["success"], true, "alert page: {alert_page}");
    thread::sleep(Duration::from_millis(250));
    let dismissed = request(&client, "dialog.dismiss", json!({}));
    assert_eq!(dismissed["success"], true, "dialog dismiss: {dismissed}");
    assert_eq!(
        dismissed["data"],
        json!({"handled":true,"action":"dismiss"})
    );
    let no_pending = request(&client, "dialog.dismiss", json!({}));
    assert_eq!(
        no_pending["success"], false,
        "no-pending dismiss: {no_pending}"
    );

    let auto = request(&client, "dialog.auto", json!({"mode":"dismiss"}));
    assert_eq!(auto["success"], true, "dialog auto: {auto}");
    assert_eq!(auto["data"], json!({"auto_mode":"dismiss"}));
    let auto_alert = request(
        &client,
        "open",
        json!({"url":"data:text/html,%3Cscript%3EsetTimeout(()%3D%3Ealert('auto%20dismiss')%2C100)%3C%2Fscript%3E"}),
    );
    assert_eq!(auto_alert["success"], true, "auto alert page: {auto_alert}");
    thread::sleep(Duration::from_millis(250));
    let auto_status = request(&client, "dialog.status", json!({}));
    assert_eq!(auto_status["success"], true, "auto status: {auto_status}");
    assert_eq!(auto_status["data"]["handled"], true);
    assert_eq!(auto_status["data"]["auto_mode"], "dismiss");
    let auto_off = request(&client, "dialog.auto", json!({"mode":"off"}));
    assert_eq!(auto_off["success"], true, "dialog auto off: {auto_off}");
    assert_eq!(auto_off["data"], json!({"auto_mode":"off"}));

    let unsupported = request(&client, "network.har", json!({}));
    assert_eq!(unsupported["success"], false);
    assert_eq!(unsupported["error"]["code"], "unsupported");

    // Stop the production accept loop before removing this test's private state.
    server.stop();
    let _ = thread.join();
    // A configured session profile is persistent by contract. The daemon must
    // not delete it; only the engine's internally-created temporary profile is
    // removable on close.
    assert!(profile.exists(), "configured session profile disappeared");
    if let Some(directory) = private_socket_dir {
        fs::remove_dir_all(directory).expect("remove private socket directory");
    }
    let _ = fs::remove_dir_all(root);
}
