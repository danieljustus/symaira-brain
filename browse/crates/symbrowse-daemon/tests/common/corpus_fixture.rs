//! Loopback navigation fixture shared by every native engine corpus (ENG-009,
//! ENG-010). std-only so the Firefox adapter tests can include this very file.
//!
//! Routes (everything else is 404):
//! - `/`          plain page
//! - `/redirect`  302 to `/landing` carrying `Set-Cookie: session=abc`
//! - `/landing`   page whose script sets local/session storage, a JS cookie and
//!   `document.body.dataset.ready`
//! - `/echo`      text/plain echo of the request's `Cookie` and `User-Agent`
//! - `/json`      `{"ok":true}`
//! - `/dl`        attachment download
//! - `/hang`      accepts the request and never answers (timeout probe)
#![allow(dead_code)]

use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

pub const DOWNLOAD_BODY: &[u8] = b"downloaded-by-corpus";

pub struct Fixture {
    pub base: String,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Fixture {
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// `METHOD path` lines in arrival order.
    pub fn requests(&self) -> Vec<String> {
        self.seen.lock().expect("request log").clone()
    }
}

pub fn serve() -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
    let address = listener.local_addr().expect("fixture address");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let log = Arc::clone(&log);
            thread::spawn(move || handle(stream, &log));
        }
    });
    Fixture {
        base: format!("http://{address}"),
        seen,
    }
}

fn handle(mut stream: TcpStream, log: &Mutex<Vec<String>>) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let (mut cookie, mut agent) = (String::new(), String::new());
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
            break;
        }
        let lower = header.to_ascii_lowercase();
        if lower.starts_with("cookie:") {
            cookie = header[7..].trim().to_owned();
        } else if lower.starts_with("user-agent:") {
            agent = header[11..].trim().to_owned();
        }
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_owned();
    let path = parts.next().unwrap_or("/").to_owned();
    log.lock()
        .expect("request log")
        .push(format!("{method} {path}"));
    if path == "/hang" {
        // Hold the connection open without answering until the test process
        // ends; the engine must give up on its own deadline.
        thread::sleep(Duration::from_secs(120));
        return;
    }
    let (status, content_type, extra, body): (&str, &str, String, Vec<u8>) = match path.as_str() {
        "/" => ("200 OK", "text/html", String::new(), b"<title>home</title><h1>home</h1>".to_vec()),
        "/redirect" => (
            "302 Found",
            "text/plain",
            "Location: /landing\r\nSet-Cookie: session=abc; Path=/\r\n".into(),
            b"moved".to_vec(),
        ),
        "/landing" => (
            "200 OK",
            "text/html",
            String::new(),
            b"<title>landing</title><p id=hello>landed</p><script>localStorage.setItem('k','v');sessionStorage.setItem('s','1');document.cookie='js=1; path=/';document.body.dataset.ready='yes';</script>".to_vec(),
        ),
        "/echo" => (
            "200 OK",
            "text/plain",
            String::new(),
            format!("cookie={cookie};ua={agent}").into_bytes(),
        ),
        "/json" => ("200 OK", "application/json", String::new(), br#"{"ok":true}"#.to_vec()),
        "/dl" => (
            "200 OK",
            "application/octet-stream",
            "Content-Disposition: attachment; filename=\"corpus.txt\"\r\n".into(),
            DOWNLOAD_BODY.to_vec(),
        ),
        _ => ("404 Not Found", "text/plain", String::new(), b"missing".to_vec()),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
}
