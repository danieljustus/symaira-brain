//! Production-daemon Chrome suite for ENG-005/006/007 (RUST-012): every
//! operation goes through the Unix-socket daemon against a real, isolated
//! Chrome. Each executed parity case prints `PARITY_CASE <id> PASS`; the
//! harness (`run.py --suite chrome-daemon`) requires the full expected set, so a
//! skipped or partial run is never evidence.
#![cfg(unix)]
#![deny(unsafe_code)]

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    os::unix::net::UnixStream,
    path::PathBuf,
    thread,
    time::Duration,
};

mod common;

use serde_json::{Value, json};
use symbrowse_daemon::{Frame, Server, ServerOptions, SessionSpec};

/// SHA-256 of `DOWNLOAD_BODY`, computed independently of the daemon.
const DOWNLOAD_BODY: &[u8] = b"downloaded-by-daemon";
const DOWNLOAD_SHA256: &str = "6f05c819223207bada3ec8fad4c3c9c452ed782589d09fed4a536ebb3cac4a21";

fn case(id: &str) {
    println!("PARITY_CASE {id} PASS");
}

/// Persistent loopback fixture: `/` page, `/form` (upload input + download
/// link), `/dl` (attachment), `/blocked/x`; anything else is 404.
fn serve_fixture() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
    let address = listener.local_addr().expect("fixture address");
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    return;
                }
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                        break;
                    }
                }
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let (status, content_type, extra, body): (&str, &str, &str, Vec<u8>) =
                    match path.as_str() {
                        "/" => ("200 OK", "text/html", "", b"<h1>network</h1>".to_vec()),
                        "/entry" => (
                            "200 OK",
                            "text/html",
                            "",
                            b"<title>entry</title><input id=name><input id=key><button id=go onclick=\"document.getElementById('out').textContent='clicked';document.title='clicked'\">Go</button><div id=out></div><a id=link href=#there>link</a><div style='height:3000px'></div><p id=bottom>bottom</p>".to_vec(),
                        ),
                        "/form" => (
                            "200 OK",
                            "text/html",
                            "",
                            b"<title>form</title><input id=file type=file><a id=dl href=/dl>download</a>"
                                .to_vec(),
                        ),
                        "/dl" => (
                            "200 OK",
                            "application/octet-stream",
                            "Content-Disposition: attachment; filename=\"daemon.txt\"\r\n",
                            DOWNLOAD_BODY.to_vec(),
                        ),
                        "/blocked/x" => ("200 OK", "text/html", "", b"<h1>blocked</h1>".to_vec()),
                        _ => ("404 Not Found", "text/plain", "", b"missing".to_vec()),
                    };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            });
        }
    });
    format!("http://{address}")
}

/// Send a `confirm`-class command and approve its out-of-band prompt, the way a
/// human would through `oob.complete` (production gate path, not a bypass).
fn request_approved(socket: &PathBuf, command: &str, args: Value) -> Value {
    let worker_socket = socket.clone();
    let frame_command = command.to_owned();
    let worker = thread::spawn(move || request(&worker_socket, &frame_command, args));
    for _ in 0..100 {
        thread::sleep(Duration::from_millis(100));
        let status = request(socket, "oob.status", json!({}));
        if let Some(id) = status["data"]["prompt"]["id"].as_str() {
            let done = request(socket, "oob.complete", json!({"id": id}));
            assert_eq!(done["success"], true, "oob.complete: {done}");
            break;
        }
    }
    worker.join().expect("approval worker")
}

fn artifact_bytes(response: &Value) -> Vec<u8> {
    response["data"]["bytes"]
        .as_array()
        .expect("artifact bytes")
        .iter()
        .map(|byte| byte.as_u64().expect("byte") as u8)
        .collect()
}

fn enabled() -> bool {
    std::env::var_os("SYMBROWSE_E2E").as_deref() == Some(std::ffi::OsStr::new("1"))
}

fn request(socket: &PathBuf, command: &str, args: Value) -> Value {
    let mut stream = UnixStream::connect(socket).expect("connect daemon");
    let frame = Frame {
        cmd: command.to_owned(),
        args: Some(args),
        session: "native-chrome".into(),
        ..Frame::default()
    };
    let line = serde_json::to_string(&frame).expect("encode frame");
    writeln!(stream, "{line}").expect("write frame");
    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .expect("read response");
    serde_json::from_str(&response).expect("decode response")
}

#[test]
fn production_daemon_path_runs_chrome_and_reaps_owned_profile() {
    if !enabled() {
        return;
    }
    // The daemon reads HOME/XDG at startup; run in a re-executed process whose
    // environment points at an isolated root, so no user cache or profile is used.
    // CLI (TTY) mode keeps Upload/Download at `confirm`, needing approval.
    let Some(root) = common::isolated_environment_root(
        "symbrowse-daemon-chrome",
        "production_daemon_path_runs_chrome_and_reaps_owned_profile",
    ) else {
        return;
    };
    let mut spec = SessionSpec::for_session("native-chrome");
    spec.state_dir = root.join("state");
    spec.cache_dir = root.join("cache");
    // Unix socket paths are limited to ~100 bytes; TMPDIR may be longer.
    spec.socket_path = PathBuf::from(format!("/tmp/sbcn-{}.sock", std::process::id()));
    spec.operation_timeout = Duration::from_secs(60);
    spec.idle_timeout = Some(Duration::from_secs(30));
    // The loopback fixture is a private address.
    spec.allow_private = true;
    let profile = spec.user_data_dir();

    let server = std::sync::Arc::new(
        Server::new(ServerOptions {
            session_spec: Some(spec.clone()),
            operation_timeout: Duration::from_secs(60),
            idle_timeout: Some(Duration::from_secs(30)),
            ..ServerOptions::default()
        })
        .expect("create daemon"),
    );
    let socket = spec.socket_path.clone();
    let server_for_thread = std::sync::Arc::clone(&server);
    let thread = thread::spawn(move || server_for_thread.listen_and_serve().expect("serve daemon"));
    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(socket.exists(), "daemon socket did not appear");

    let capabilities = request(&socket, "capabilities", json!({}));
    assert_eq!(capabilities["success"], true);
    let opened = request(
        &socket,
        "open",
        json!({"url": "data:text/html,<title>daemon</title><h1>native</h1>"}),
    );
    assert_eq!(opened["success"], true, "open response: {opened}");
    let script = request(&socket, "read", json!({}));
    assert!(
        script["data"]
            .as_str()
            .is_some_and(|text| text.contains("native")),
        "read response: {script}"
    );
    case("ENG-005.navigate-read");
    let tabs = request(&socket, "tabs.list", json!({}));
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
        &socket,
        "tab.new",
        json!({"label":"second","url":"data:text/html,<h1>second</h1>"}),
    );
    assert_eq!(created["success"], true, "tab.new response: {created}");
    assert_eq!(created["data"]["tab"], "t2");
    assert_eq!(created["data"]["label"], "second");
    let listed_tabs = request(&socket, "tab.list", json!({}));
    assert_eq!(
        listed_tabs["success"], true,
        "tab.list response: {listed_tabs}"
    );
    assert_eq!(
        listed_tabs["data"]["tabs"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(listed_tabs["data"]["active"], "t2");
    let switched = request(&socket, "tab.switch", json!({"tab":"t1"}));
    assert_eq!(switched["success"], true, "tab.switch response: {switched}");
    let original = request(&socket, "read", json!({}));
    assert!(
        original["data"]
            .as_str()
            .is_some_and(|text| text.contains("native")),
        "switched tab read: {original}"
    );
    let closed = request(&socket, "tab.close", json!({"tab":"second"}));
    assert_eq!(closed["success"], true, "tab.close response: {closed}");
    assert_eq!(closed["data"]["closed"], "t2");
    assert_eq!(closed["data"]["active"], "t1");
    let remaining = request(&socket, "tab.list", json!({}));
    assert_eq!(remaining["data"]["tabs"].as_array().map(Vec::len), Some(1));
    let window = request(&socket, "window.new", json!({}));
    assert_eq!(window["success"], true, "window.new response: {window}");
    assert_eq!(window["data"]["tab"], "t2");
    let closed_window = request(&socket, "tab.close", json!({}));
    assert_eq!(
        closed_window["success"], true,
        "close active tab: {closed_window}"
    );
    case("ENG-006.tabs");
    let base = serve_fixture();
    let started = request(&socket, "network.capture", json!({}));
    assert_eq!(started["success"], true, "network capture start: {started}");
    assert_eq!(started["data"]["started"], true);
    let network_page = request(&socket, "open", json!({"url": format!("{base}/")}));
    assert_eq!(
        network_page["success"], true,
        "network page: {network_page}"
    );
    let captured = request(&socket, "network.requests", json!({}));
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
    case("ENG-007.network-capture");
    let framed = request(
        &socket,
        "open",
        json!({"url": "data:text/html,<iframe id='outer' srcdoc=\"<iframe id='inner' srcdoc='nested'></iframe>\"></iframe>"}),
    );
    assert_eq!(framed["success"], true, "frame page: {framed}");
    let frames = request(&socket, "frame.tree", json!({}));
    assert_eq!(frames["success"], true, "frame response: {frames}");
    // Go's frame.tree returns a nested tree (`children`), not a flat list.
    fn flatten<'a>(nodes: &'a [Value], out: &mut Vec<&'a Value>) {
        for node in nodes {
            out.push(node);
            if let Some(children) = node["children"].as_array() {
                flatten(children, out);
            }
        }
    }
    let mut listed = Vec::new();
    flatten(
        frames["data"]["frames"].as_array().expect("frame tree"),
        &mut listed,
    );
    assert!(listed.len() >= 3, "frame response: {frames}");
    assert!(
        listed.iter().any(|frame| frame["parent_id"].is_string()),
        "frame response: {frames}"
    );
    case("ENG-006.frames");
    let ax = request(&socket, "a11y", json!({}));
    assert_eq!(ax["success"], true, "a11y response: {ax}");
    case("ENG-007.a11y");

    let interactions = request(
        &socket,
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
        let response = request(&socket, command, args);
        assert_eq!(response["success"], true, "{command} response: {response}");
        assert_eq!(response["data"]["action"], command);
        if command == "focus" {
            let focused = request(
                &socket,
                "get.attr",
                json!({"selector":"#text","attribute":"data-focused"}),
            );
            assert_eq!(focused["data"], "yes", "focus state: {focused}");
        }
        if command == "check" {
            let checked = request(&socket, "is.checked", json!({"selector":"#check"}));
            assert_eq!(checked["data"], true, "check state: {checked}");
        }
    }
    let doubled = request(
        &socket,
        "get.attr",
        json!({"selector":"#dbl","attribute":"data-doubled"}),
    );
    assert_eq!(doubled["data"], "yes", "double-click state: {doubled}");
    let hovered = request(
        &socket,
        "get.attr",
        json!({"selector":"#hover","attribute":"data-hovered"}),
    );
    assert_eq!(hovered["data"], "yes", "hover state: {hovered}");
    let selected = request(&socket, "get.value", json!({"selector":"#choice"}));
    assert_eq!(selected["data"], "two", "select state: {selected}");
    let checked = request(&socket, "is.checked", json!({"selector":"#check"}));
    assert_eq!(checked["data"], false, "uncheck state: {checked}");
    case("ENG-005.interactions");

    // Dialogs fire 1s after load so a slow readyState poll on a loaded host
    // cannot race the blocking alert (evaluate hangs while a dialog is open).
    let no_dialog = request(&socket, "dialog.status", json!({}));
    assert_eq!(
        no_dialog["success"], true,
        "empty dialog status: {no_dialog}"
    );
    assert_eq!(no_dialog["data"]["handled"], true);
    let prompt_page = request(
        &socket,
        "open",
        json!({"url":"data:text/html,%3Cscript%3EsetTimeout(()%3D%3Eprompt('native%20prompt'%2C'seed')%2C1000)%3C%2Fscript%3E"}),
    );
    assert_eq!(prompt_page["success"], true, "prompt page: {prompt_page}");
    thread::sleep(Duration::from_millis(1300));
    let prompt_status = request(&socket, "dialog.status", json!({}));
    assert_eq!(
        prompt_status["success"], true,
        "prompt status: {prompt_status}"
    );
    assert_eq!(prompt_status["data"]["type"], "prompt");
    assert_eq!(prompt_status["data"]["message"], "native prompt");
    assert_eq!(prompt_status["data"]["default"], "seed");
    assert_eq!(prompt_status["data"]["handled"], false);
    let accepted = request(&socket, "dialog.accept", json!({"text":"answer"}));
    assert_eq!(accepted["success"], true, "dialog accept: {accepted}");
    assert_eq!(accepted["data"], json!({"handled":true,"action":"accept"}));
    let handled = request(&socket, "dialog.status", json!({}));
    assert_eq!(
        handled["data"]["handled"], true,
        "handled status: {handled}"
    );
    case("ENG-006.dialogs-manual");

    let alert_page = request(
        &socket,
        "open",
        json!({"url":"data:text/html,%3Cscript%3EsetTimeout(()%3D%3Ealert('native%20alert')%2C1000)%3C%2Fscript%3E"}),
    );
    assert_eq!(alert_page["success"], true, "alert page: {alert_page}");
    thread::sleep(Duration::from_millis(1300));
    let dismissed = request(&socket, "dialog.dismiss", json!({}));
    assert_eq!(dismissed["success"], true, "dialog dismiss: {dismissed}");
    assert_eq!(
        dismissed["data"],
        json!({"handled":true,"action":"dismiss"})
    );
    let no_pending = request(&socket, "dialog.dismiss", json!({}));
    assert_eq!(
        no_pending["success"], false,
        "no-pending dismiss: {no_pending}"
    );

    let auto = request(&socket, "dialog.auto", json!({"mode":"dismiss"}));
    assert_eq!(auto["success"], true, "dialog auto: {auto}");
    assert_eq!(auto["data"], json!({"auto_mode":"dismiss"}));
    let auto_alert = request(
        &socket,
        "open",
        json!({"url":"data:text/html,%3Cscript%3EsetTimeout(()%3D%3Ealert('auto%20dismiss')%2C1000)%3C%2Fscript%3E"}),
    );
    assert_eq!(auto_alert["success"], true, "auto alert page: {auto_alert}");
    thread::sleep(Duration::from_millis(1300));
    let auto_status = request(&socket, "dialog.status", json!({}));
    assert_eq!(auto_status["success"], true, "auto status: {auto_status}");
    assert_eq!(auto_status["data"]["handled"], true);
    assert_eq!(auto_status["data"]["auto_mode"], "dismiss");
    let auto_off = request(&socket, "dialog.auto", json!({"mode":"off"}));
    assert_eq!(auto_off["success"], true, "dialog auto off: {auto_off}");
    assert_eq!(auto_off["data"], json!({"auto_mode":"off"}));
    case("ENG-006.dialogs-auto");

    // ---- ENG-005: text entry, click, inspection, waits and history ----------
    let form_page = request(&socket, "open", json!({"url": format!("{base}/entry")}));
    assert_eq!(form_page["success"], true, "form page: {form_page}");
    for (command, args) in [
        ("fill", json!({"selector":"#name","value":"alice"})),
        ("type", json!({"selector":"#key","value":"bob"})),
        ("press", json!({"selector":"#key","key":"Enter"})),
        ("click", json!({"selector":"#go"})),
        ("scrollintoview", json!({"selector":"#bottom"})),
    ] {
        let response = request(&socket, command, args);
        assert_eq!(response["success"], true, "{command} response: {response}");
    }
    let filled = request(&socket, "get.value", json!({"selector":"#name"}));
    assert_eq!(filled["data"], "alice", "fill state: {filled}");
    let typed = request(&socket, "get.value", json!({"selector":"#key"}));
    assert_eq!(typed["data"], "bob", "type state: {typed}");
    let out = request(&socket, "get.text", json!({"selector":"#out"}));
    assert_eq!(out["data"], "clicked", "click state: {out}");
    case("ENG-005.form-input");

    let title = request(&socket, "get.title", json!({}));
    assert_eq!(title["data"], "clicked", "title: {title}");
    for (command, args) in [
        ("get.html", json!({"selector":"#out"})),
        ("get.count", json!({"selector":"input"})),
        ("get.box", json!({"selector":"#go"})),
        ("get.styles", json!({"selector":"#go"})),
        ("is.visible", json!({"selector":"#go"})),
        ("is.enabled", json!({"selector":"#go"})),
        ("find", json!({"kind":"text","query":"Go"})),
    ] {
        let response = request(&socket, command, args);
        assert_eq!(response["success"], true, "{command} response: {response}");
    }
    let count = request(&socket, "get.count", json!({"selector":"input"}));
    assert_eq!(count["data"], 2, "count: {count}");
    let visible = request(&socket, "is.visible", json!({"selector":"#go"}));
    assert_eq!(visible["data"], true, "visible: {visible}");
    let waited = request(&socket, "wait", json!({"kind":"selector","value":"#out"}));
    assert_eq!(waited["success"], true, "wait selector: {waited}");
    let waited = request(&socket, "wait", json!({"kind":"text","value":"clicked"}));
    assert_eq!(waited["success"], true, "wait text: {waited}");
    let navigated = request(&socket, "click", json!({"selector":"#link"}));
    assert_eq!(navigated["success"], true, "hash click: {navigated}");
    let waited = request(&socket, "wait", json!({"kind":"url","value":"#there"}));
    assert_eq!(waited["success"], true, "wait url: {waited}");
    let missing = request(&socket, "click", json!({"selector":"#does-not-exist"}));
    assert_eq!(missing["success"], false, "missing selector: {missing}");
    let reloaded = request(&socket, "reload", json!({}));
    assert_eq!(reloaded["success"], true, "reload: {reloaded}");
    case("ENG-005.inspect-find-wait");

    // ---- ENG-007: offline and URL blocking -----------------------------------
    let offline = request(&socket, "network.offline", json!({"offline": true}));
    assert_eq!(offline["success"], true, "offline on: {offline}");
    let refused = request(&socket, "open", json!({"url": format!("{base}/")}));
    assert_eq!(refused["success"], false, "offline navigation: {refused}");
    let online = request(&socket, "network.offline", json!({"offline": false}));
    assert_eq!(online["success"], true, "offline off: {online}");
    let restored = request(&socket, "open", json!({"url": format!("{base}/")}));
    assert_eq!(restored["success"], true, "online navigation: {restored}");
    let probe = format!("fetch('{base}/blocked/x').then(() => 'loaded', () => 'blocked')");
    let blocked = request(
        &socket,
        "network.block",
        json!({"urls":[format!("{base}/blocked/*")]}),
    );
    assert_eq!(blocked["success"], true, "block: {blocked}");
    // Chrome applies Network.setBlockedURLs to subresource requests only (a
    // main-frame navigation to a blocked URL still commits), so probe with a
    // same-origin fetch from the already loaded page.
    let refused = request_approved(&socket, "eval", json!({"expression": probe}));
    assert_eq!(refused["success"], true, "blocked fetch eval: {refused}");
    assert!(
        refused["data"].to_string().contains("blocked"),
        "blocked fetch: {refused}"
    );
    let unblocked = request(&socket, "network.block", json!({"urls":[]}));
    assert_eq!(unblocked["success"], true, "unblock: {unblocked}");
    let reloaded_fetch = request_approved(&socket, "eval", json!({"expression": probe}));
    assert!(
        reloaded_fetch["data"].to_string().contains("loaded"),
        "unblocked fetch: {reloaded_fetch}"
    );
    case("ENG-007.network-offline-block");

    // ---- ENG-007: HAR start/stop ---------------------------------------------
    let har_start = request(&socket, "network.har", json!({"action":"start"}));
    assert_eq!(har_start["success"], true, "har start: {har_start}");
    let har_page = request(&socket, "open", json!({"url": format!("{base}/")}));
    assert_eq!(har_page["success"], true, "har page: {har_page}");
    let har_stop = request(&socket, "network.har", json!({"action":"stop"}));
    assert_eq!(har_stop["success"], true, "har stop: {har_stop}");
    assert!(
        har_stop["data"]["entries"].as_u64().is_some_and(|n| n > 0),
        "har entries: {har_stop}"
    );
    assert!(
        har_stop["data"]["har"]["log"]["entries"]
            .as_array()
            .is_some_and(|entries| entries
                .iter()
                .any(|entry| entry["request"]["url"] == format!("{base}/"))),
        "har content: {har_stop}"
    );
    case("ENG-007.network-har");

    // ---- ENG-007: screenshots and PDF ----------------------------------------
    let png = request(&socket, "screenshot", json!({}));
    assert_eq!(png["success"], true, "png: {}", png.to_string().len());
    assert_eq!(png["data"]["mime_type"], "image/png");
    assert_eq!(&artifact_bytes(&png)[..4], &[0x89, b'P', b'N', b'G']);
    let jpeg = request(&socket, "screenshot", json!({"format":"jpeg","quality":60}));
    assert_eq!(jpeg["success"], true, "jpeg: {}", jpeg.to_string().len());
    assert_eq!(jpeg["data"]["mime_type"], "image/jpeg");
    assert_eq!(&artifact_bytes(&jpeg)[..3], &[0xff, 0xd8, 0xff]);
    let full = request(&socket, "screenshot", json!({"full_page": true}));
    assert_eq!(
        full["success"],
        true,
        "full page: {}",
        full.to_string().len()
    );
    assert_eq!(&artifact_bytes(&full)[..4], &[0x89, b'P', b'N', b'G']);
    let element = request(&socket, "screenshot", json!({"selector":"h1"}));
    assert_eq!(
        element["success"],
        true,
        "selector: {}",
        element.to_string().len()
    );
    assert!(
        artifact_bytes(&element).len() < artifact_bytes(&png).len(),
        "an element capture must be smaller than the viewport capture"
    );
    let bad_format = request(&socket, "screenshot", json!({"format":"gif"}));
    assert_eq!(
        bad_format["success"], false,
        "unsupported format: {bad_format}"
    );
    case("ENG-007.screenshot");
    let pdf = request(&socket, "pdf", json!({}));
    assert_eq!(pdf["success"], true, "pdf: {}", pdf.to_string().len());
    assert_eq!(pdf["data"]["mime_type"], "application/pdf");
    assert_eq!(&artifact_bytes(&pdf)[..5], b"%PDF-");
    case("ENG-007.pdf");

    // ---- ENG-007: upload (allow-listed) and download (GUID + checksum) -------
    let form = request(&socket, "open", json!({"url": format!("{base}/form")}));
    assert_eq!(form["success"], true, "form page: {form}");
    let uploads = root.join("uploads");
    fs::create_dir_all(&uploads).expect("create upload directory");
    let upload_file = uploads.join("upload.txt");
    fs::write(&upload_file, b"upload-body").expect("write upload fixture");
    let uploaded = request_approved(
        &socket,
        "upload",
        json!({"selector":"#file","files":[upload_file.to_string_lossy()],"allowed_dirs":[uploads.to_string_lossy()]}),
    );
    assert_eq!(uploaded["success"], true, "upload: {uploaded}");
    let chosen = request(&socket, "get.value", json!({"selector":"#file"}));
    assert!(
        chosen["data"]
            .as_str()
            .is_some_and(|value| value.ends_with("upload.txt")),
        "upload state: {chosen}"
    );
    let outside = root.join("outside.txt");
    fs::write(&outside, b"secret").expect("write outside fixture");
    let rejected = request_approved(
        &socket,
        "upload",
        json!({"selector":"#file","files":[outside.to_string_lossy()],"allowed_dirs":[uploads.to_string_lossy()]}),
    );
    assert_eq!(rejected["success"], false, "outside upload: {rejected}");
    case("ENG-007.upload");

    let empty = request(&socket, "downloads.list", json!({}));
    assert_eq!(empty["data"]["count"], 0, "no downloads yet: {empty}");
    let downloads_dir = root.join("downloads");
    let set_dir = request_approved(
        &socket,
        "download.setdir",
        json!({"dir": downloads_dir.to_string_lossy()}),
    );
    assert_eq!(set_dir["success"], true, "setdir: {set_dir}");
    assert_eq!(
        PathBuf::from(
            set_dir["data"]["download_dir"]
                .as_str()
                .expect("download_dir")
        )
        .canonicalize()
        .expect("canonical download dir"),
        downloads_dir
            .canonicalize()
            .expect("canonical expected dir")
    );
    let clicked = request(&socket, "click", json!({"selector":"#dl"}));
    assert_eq!(clicked["success"], true, "download click: {clicked}");
    let mut completed = Value::Null;
    for _ in 0..100 {
        let listed = request(&socket, "downloads.list", json!({}));
        if let Some(entry) = listed["data"]["downloads"]
            .as_array()
            .and_then(|entries| entries.iter().find(|e| e["state"] == "completed"))
        {
            completed = entry.clone();
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(completed.is_object(), "download did not complete");
    assert_eq!(completed["filename"], "daemon.txt");
    assert_eq!(completed["url"], format!("{base}/dl"));
    assert_eq!(completed["sha256"], DOWNLOAD_SHA256);
    let guid = completed["guid"].as_str().expect("download guid");
    assert_eq!(
        fs::read(downloads_dir.join(guid)).expect("GUID-named download file"),
        DOWNLOAD_BODY
    );
    case("ENG-007.download");

    // ---- ENG-007: explicit unsupported operations ----------------------------
    for (command, args) in [
        ("axe.audit", json!({})),
        ("download", json!({})),
        ("network.har", json!({"action":"stop","content":"all"})),
    ] {
        let response = request(&socket, command, args);
        assert_eq!(
            response["success"], false,
            "{command} must fail: {response}"
        );
    }
    let axe = request(&socket, "axe.audit", json!({}));
    assert_eq!(axe["error"]["code"], "unsupported", "axe: {axe}");
    let bare = request(&socket, "download", json!({}));
    assert_eq!(bare["error"]["code"], "unsupported", "download: {bare}");
    case("ENG-007.unsupported-explicit");

    // Go implements network.har; an empty action is rejected like Go does.
    let bad_har = request(&socket, "network.har", json!({}));
    assert_eq!(bad_har["success"], false);
    assert_eq!(bad_har["error"]["code"], "invalid_har_action");

    // The one-second idle budget lets the production accept loop stop and run
    // its owned socket/profile cleanup without touching any user browser.
    server.stop();
    let _ = thread.join();
    // A configured session profile is persistent by contract. The daemon must
    // not delete it; only the engine's internally-created temporary profile is
    // removable on close.
    assert!(profile.exists(), "configured session profile disappeared");
    let _ = fs::remove_dir_all(root);
}
