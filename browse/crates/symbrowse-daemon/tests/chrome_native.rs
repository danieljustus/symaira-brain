use std::{
    fs,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
use symbrowse_daemon::{Client, ClientOptions, Frame, Server, ServerOptions, SessionSpec};

fn enabled() -> bool {
    std::env::var_os("SYMBROWSE_E2E").as_deref() == Some(std::ffi::OsStr::new("1"))
}

fn request(client: &Client, command: &str, args: Value) -> Value {
    let frame = Frame {
        cmd: command.to_owned(),
        args: Some(args),
        session: client.options().session.clone(),
        ..Frame::default()
    };
    let response = client
        .request(frame)
        .unwrap_or_else(|error| panic!("request {command}: {error}"));
    serde_json::to_value(response).expect("encode response")
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
    let opened = request(
        &client,
        "open",
        json!({"url": "data:text/html,<title>daemon</title><h1>native</h1>"}),
    );
    assert_eq!(opened["success"], true, "open response: {opened}");
    let script = request(&client, "read", json!({}));
    assert!(
        script["data"]
            .as_str()
            .is_some_and(|text| text.contains("native")),
        "read response: {script}"
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
