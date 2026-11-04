//! Replays the frozen Go observations of `symbrain memory sync`
//! (`tests/fixtures/memory_sync/*.json`, captured by `capture.py` from the
//! shipped Go CLI; see `provenance.json`) against the native binary. No Go,
//! Go binary or Go source is needed: a scripted local fake server plays the
//! recorded responses and the binary's HTTP requests, stdout, stderr, exit
//! status and database are compared with what Go produced.
//!
//! Relay blobs carry random nonces, so request bodies are compared after
//! decryption.

#![allow(
    clippy::too_many_lines,
    clippy::format_push_string,
    clippy::large_stack_arrays,
    clippy::assigning_clones
)]

use regex::Regex;
use serde_json::{Map, Value, json};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use symbrain_memory::sync_client::RelayCipher;

const HEADERS: [&str; 6] = [
    "authorization",
    "content-type",
    "user-agent",
    "accept-encoding",
    "referer",
    "accept",
];

struct Recorded {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

struct Server {
    port: u16,
    requests: Arc<Mutex<Vec<Recorded>>>,
    responses: Arc<Mutex<Vec<Value>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

fn header_end(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|window| window == b"\r\n\r\n")
}

fn read_request(stream: &mut TcpStream) -> Option<Recorded> {
    let mut data = Vec::new();
    let mut chunk = [0_u8; 65536];
    let end = loop {
        if let Some(end) = header_end(&data) {
            break end;
        }
        let count = stream.read(&mut chunk).ok()?;
        if count == 0 {
            return None;
        }
        data.extend_from_slice(&chunk[..count]);
    };
    let head = String::from_utf8_lossy(&data[..end]).into_owned();
    let mut body = data[end + 4..].to_vec();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.splitn(3, ' ');
    let method = first.next()?.to_owned();
    let target = first.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let find = |name: &str| {
        headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, v)| v.clone())
    };
    if let Some(length) = find("content-length").and_then(|v| v.parse::<usize>().ok()) {
        while body.len() < length {
            let count = stream.read(&mut chunk).ok()?;
            if count == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..count]);
        }
    } else if find("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        let mut buffer = body;
        let mut decoded = Vec::new();
        loop {
            while !buffer.windows(2).any(|w| w == b"\r\n") {
                let count = stream.read(&mut chunk).ok()?;
                if count == 0 {
                    return None;
                }
                buffer.extend_from_slice(&chunk[..count]);
            }
            let at = buffer.windows(2).position(|w| w == b"\r\n")?;
            let size = usize::from_str_radix(
                String::from_utf8_lossy(&buffer[..at])
                    .split(';')
                    .next()?
                    .trim(),
                16,
            )
            .ok()?;
            buffer.drain(..at + 2);
            while buffer.len() < size + 2 {
                let count = stream.read(&mut chunk).ok()?;
                if count == 0 {
                    return None;
                }
                buffer.extend_from_slice(&chunk[..count]);
            }
            decoded.extend_from_slice(&buffer[..size]);
            buffer.drain(..size + 2);
            if size == 0 {
                break;
            }
        }
        body = decoded;
    }
    Some(Recorded {
        method,
        target,
        headers,
        body,
    })
}

fn reply(stream: &mut TcpStream, response: &Value, port: u16) {
    let base = format!("http://127.0.0.1:{port}");
    let substitute = |text: &str| text.replace("{base}", &base);
    let action = response["action"].as_str().unwrap_or("ok");
    if action == "drop" {
        return;
    }
    let pad = "x".repeat(usize::try_from(response["pad"].as_u64().unwrap_or(0)).unwrap());
    let mut body = substitute(response["body"].as_str().unwrap_or(""))
        .replace("{pad}", &pad)
        .into_bytes();
    let mut headers: Vec<(String, String)> = response["headers"]
        .as_object()
        .map(|map| {
            map.iter()
                .map(|(k, v)| (k.clone(), substitute(v.as_str().unwrap_or(""))))
                .collect()
        })
        .unwrap_or_default();
    if response["gzip"].as_bool() == Some(true) {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&body).unwrap();
        body = encoder.finish().unwrap();
        headers.push(("Content-Encoding".to_owned(), "gzip".to_owned()));
    }
    let status = response["status"].as_u64().unwrap_or(200);
    let mut head = format!("HTTP/1.1 {status} Status\r\n");
    for (name, value) in &headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    let stall = Duration::from_secs(response["stall"].as_u64().unwrap_or(0));
    match action {
        "stall" => {
            std::thread::sleep(stall);
            head.push_str(&format!(
                "Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            ));
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
        "short_length" => {
            head.push_str(&format!(
                "Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len() + 40
            ));
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
        "truncate" => {
            head.push_str(&format!(
                "Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len() + 100
            ));
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body[..body.len() / 2]);
        }
        "stall_body" => {
            head.push_str(&format!(
                "Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            ));
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body[..5.min(body.len())]);
            let _ = stream.flush();
            std::thread::sleep(stall);
        }
        _ if response["chunked"].as_bool() == Some(true) => {
            head.push_str("Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n");
            let mut out = head.into_bytes();
            for piece in body.chunks(7) {
                out.extend_from_slice(format!("{:x}\r\n", piece.len()).as_bytes());
                out.extend_from_slice(piece);
                out.extend_from_slice(b"\r\n");
            }
            out.extend_from_slice(b"0\r\n\r\n");
            let _ = stream.write_all(&out);
        }
        _ => {
            head.push_str(&format!(
                "Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            ));
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    }
}

impl Server {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let responses: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (seen, queue, halt) = (requests.clone(), responses.clone(), stop.clone());
        std::thread::spawn(move || {
            while !halt.load(std::sync::atomic::Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let (seen, queue) = (seen.clone(), queue.clone());
                        std::thread::spawn(move || {
                            stream.set_nonblocking(false).unwrap();
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
                            let Some(request) = read_request(&mut stream) else {
                                return;
                            };
                            seen.lock().unwrap().push(request);
                            let response = {
                                let mut queue = queue.lock().unwrap();
                                if queue.is_empty() {
                                    json!({"status": 500, "headers": {"Content-Type": "application/json"}, "body": "{\"error\": \"unscripted request\"}"})
                                } else {
                                    queue.remove(0)
                                }
                            };
                            reply(&mut stream, &response, port);
                        });
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Self {
            port,
            requests,
            responses,
            stop,
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

fn sanitize(text: &str, port: u16, closed_port: u16, root: &str) -> String {
    let text = text.replace(root, "{root}");
    let trace = Regex::new(r"goroutine \d+ \[running\]:[\s\S]*").unwrap();
    let text = trace.replace_all(&text, "goroutine N [running]:\n<trace>\n");
    let signal = Regex::new(r"\[signal [^\]]*\]").unwrap();
    let text = signal.replace_all(&text, "[signal SIGSEGV]");
    text.replace(&format!(":{closed_port}"), ":{CLOSED}")
        .replace(&format!(":{port}"), ":{PORT}")
}

fn normalize_request(request: &Recorded, port: u16, passphrase: Option<&str>) -> Value {
    let mut headers = Map::new();
    for (name, value) in &request.headers {
        if HEADERS.contains(&name.as_str()) {
            let value = if name == "referer" {
                value.replace(&port.to_string(), "{PORT}")
            } else {
                value.clone()
            };
            headers.insert(name.clone(), Value::String(value));
        }
    }
    let host = request
        .headers
        .iter()
        .find(|(name, _)| name == "host")
        .map_or("", |(_, value)| value.as_str());
    headers.insert(
        "host".to_owned(),
        Value::String(host.replace(&port.to_string(), "{PORT}")),
    );
    let mut out = Map::new();
    out.insert("method".into(), Value::String(request.method.clone()));
    out.insert("target".into(), Value::String(request.target.clone()));
    out.insert("headers".into(), Value::Object(headers));
    let body = String::from_utf8_lossy(&request.body).into_owned();
    if let Some(passphrase) = passphrase
        && request.target.contains("/api/sync/relay")
        && request.method == "POST"
    {
        let parsed: Value = serde_json::from_str(&body).unwrap();
        let mut cipher = RelayCipher::new();
        let blobs: Vec<Value> = parsed["blobs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|blob| {
                let raw = base64_decode(blob["blob"].as_str().unwrap());
                let plain = cipher.decrypt(&raw, passphrase.as_bytes()).unwrap();
                json!({"id": blob["id"], "updated_at": blob["updated_at"], "payload": String::from_utf8(plain).unwrap()})
            })
            .collect();
        out.insert("relay_blobs".into(), Value::Array(blobs));
    } else {
        out.insert(
            "body".into(),
            Value::String(body.replace(&port.to_string(), "{PORT}")),
        );
    }
    Value::Object(out)
}

fn base64_decode(text: &str) -> Vec<u8> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut bits = 0_u32;
    let mut count = 0;
    for byte in text.bytes().filter(|byte| *byte != b'=') {
        let value = TABLE.iter().position(|entry| *entry == byte).unwrap();
        bits = (bits << 6) | u32::try_from(value).unwrap();
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push(u8::try_from((bits >> count) & 0xff).unwrap());
        }
    }
    out
}

fn normalize_cell(value: rusqlite::types::ValueRef<'_>) -> Value {
    use rusqlite::types::ValueRef;
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(number) => json!(number),
        ValueRef::Real(number) => Value::String(format!("real:{:016x}", number.to_bits())),
        ValueRef::Blob(bytes) => {
            use std::fmt::Write as _;
            let mut hex = String::from("hex:");
            for byte in bytes {
                let _ = write!(hex, "{byte:02x}");
            }
            Value::String(hex)
        }
        ValueRef::Text(bytes) => {
            let text = String::from_utf8_lossy(bytes).into_owned();
            let pattern = Regex::new(r"^(\d{4}-\d\d-\d\d)[ T](\d\d:\d\d:\d\d)").unwrap();
            if let Some(found) = pattern.captures(&text)
                && let Ok(stamp) = chrono::NaiveDateTime::parse_from_str(
                    &format!("{} {}", &found[1], &found[2]),
                    "%Y-%m-%d %H:%M:%S",
                )
                && (chrono::Utc::now().naive_utc() - stamp).num_seconds().abs() < 86_400
            {
                return Value::String("<now>".to_owned());
            }
            Value::String(text)
        }
    }
}

fn dump_db(path: &Path) -> Value {
    let Ok(conn) =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return Value::Null;
    };
    let mut out = Map::new();
    for (table, order) in [
        ("memories", "id"),
        ("sync_state", "remote"),
        ("sync_oplog", "event_id"),
    ] {
        let mut statement = conn
            .prepare(&format!("SELECT * FROM {table} ORDER BY {order}"))
            .unwrap();
        let names: Vec<String> = statement
            .column_names()
            .iter()
            .map(|n| (*n).to_owned())
            .collect();
        let mut rows = statement.query([]).unwrap();
        let mut list = Vec::new();
        while let Some(row) = rows.next().unwrap() {
            let mut object = Map::new();
            for (index, name) in names.iter().enumerate() {
                object.insert(name.clone(), normalize_cell(row.get_ref(index).unwrap()));
            }
            list.push(Value::Object(object));
        }
        out.insert(table.to_owned(), Value::Array(list));
    }
    let mut statement = conn
        .prepare("SELECT action, memory_id, scope, session, actor, detail FROM audit_log ORDER BY action, memory_id")
        .unwrap();
    let mut rows = statement.query([]).unwrap();
    let mut audit = Vec::new();
    while let Some(row) = rows.next().unwrap() {
        audit.push(Value::Array(
            (0..6)
                .map(|i| normalize_cell(row.get_ref(i).unwrap()))
                .collect(),
        ));
    }
    out.insert("audit_log".to_owned(), Value::Array(audit));
    Value::Object(out)
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/memory_sync")
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn run_fixture(fixture: &Value, scratch: &Path) -> Vec<String> {
    let name = fixture["name"].as_str().unwrap();
    let root = scratch.join(name);
    for dir in [
        "home", "config", "data", "cache", "tmp", "project", "custom",
    ] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    let root_text = root.to_str().unwrap().to_owned();
    let env: Vec<(&str, String)> = vec![
        ("PATH", "/usr/bin:/bin".into()),
        ("HOME", format!("{root_text}/home")),
        ("USERPROFILE", format!("{root_text}/home")),
        ("XDG_CONFIG_HOME", format!("{root_text}/config")),
        ("XDG_DATA_HOME", format!("{root_text}/data")),
        ("XDG_CACHE_HOME", format!("{root_text}/cache")),
        ("TMPDIR", format!("{root_text}/tmp")),
        ("LANG", "C.UTF-8".into()),
        ("LC_ALL", "C.UTF-8".into()),
        ("TZ", "UTC".into()),
    ];
    let binary = env!("CARGO_BIN_EXE_symbrain");
    let command = |args: &[String], extra: &[(String, String)]| {
        let mut command = Command::new(binary);
        command.env_clear();
        command.envs(std::env::var_os("LLVM_PROFILE_FILE").map(|path| ("LLVM_PROFILE_FILE", path)));
        command.envs(env.iter().map(|(k, v)| (*k, v.clone())));
        command.envs(extra.iter().cloned());
        command.current_dir(root.join("project")).args(args);
        command.output().unwrap()
    };
    // The schema comes from the binary under test, as in the capture.
    command(&["memory".to_owned(), "list".to_owned()], &[]);
    let db = root.join("data/symbrain/memory/default.db");
    let server = Server::start();
    let closed_port = free_port();
    let base = format!("http://127.0.0.1:{}", server.port);
    if let Some(seed) = fixture["seed"].as_array()
        && !seed.is_empty()
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        for sql in seed {
            conn.execute(&sql.as_str().unwrap().replace("{base}", &base), [])
                .unwrap();
        }
    }
    if let Some(files) = fixture["files"].as_object() {
        for (relative, content) in files {
            let target = root.join(relative.replace("{root}", &root_text));
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, content.as_str().unwrap()).unwrap();
        }
    }
    let mut problems = Vec::new();
    let substitute = |text: &str| {
        text.replace("{base}", &base)
            .replace("{port}", &server.port.to_string())
            .replace("{closed_port}", &closed_port.to_string())
            .replace("{root}", &root_text)
    };
    let mut uses_custom_db = false;
    for (index, step) in fixture["steps"].as_array().unwrap().iter().enumerate() {
        *server.responses.lock().unwrap() = step["responses"].as_array().unwrap().clone();
        server.requests.lock().unwrap().clear();
        let argv: Vec<String> = step["argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| substitute(arg.as_str().unwrap()))
            .collect();
        uses_custom_db |= step["argv"].as_array().unwrap().iter().any(|a| a == "--db");
        let extra: Vec<(String, String)> = step["env"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
            .collect();
        let output = command(&argv, &extra);
        std::thread::sleep(Duration::from_millis(50));
        let passphrase = step["passphrase"].as_str();
        let requests: Vec<Value> = server
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| normalize_request(request, server.port, passphrase))
            .collect();
        let got = json!({
            "argv": step["argv"], "env": step["env"], "passphrase": step["passphrase"],
            "requests": requests,
            "stdout": sanitize(&String::from_utf8_lossy(&output.stdout), server.port, closed_port, &root_text),
            "stderr": sanitize(&String::from_utf8_lossy(&output.stderr), server.port, closed_port, &root_text),
            "exit": output.status.code().unwrap_or(-1),
        });
        let want = &host_kernel_errno(&fixture["observed"][index]);
        if &got != want {
            problems.push(format!(
                "{name} step {index}\n want: {}\n  got: {}",
                serde_json::to_string(want).unwrap(),
                serde_json::to_string(&got).unwrap()
            ));
        }
    }
    let db_path = if uses_custom_db {
        root.join("custom/other.db")
    } else {
        db
    };
    let mut dump = dump_db(&db_path);
    if !dump.is_null() {
        let text = serde_json::to_string(&dump)
            .unwrap()
            .replace(&base, "http://127.0.0.1:{PORT}")
            .replace(&server.port.to_string(), "{PORT}");
        dump = serde_json::from_str(&text).unwrap();
    }
    if dump != fixture["db"] {
        problems.push(format!(
            "{name} database\n want: {}\n  got: {}",
            serde_json::to_string(&fixture["db"]).unwrap(),
            serde_json::to_string(&dump).unwrap()
        ));
    }
    problems
}

/// The Go observations were recorded on Darwin, where dialing 127.0.0.1:0
/// fails with EADDRNOTAVAIL. Linux answers the same dial with ECONNREFUSED,
/// which Go prints as "connection refused"; the difference is the kernel's,
/// not the implementation's.
fn host_kernel_errno(want: &Value) -> Value {
    let mut want = want.clone();
    if !cfg!(target_os = "macos")
        && let Some(stderr) = want["stderr"].as_str()
    {
        want["stderr"] = json!(stderr.replace(
            "connect: can't assign requested address",
            "connect: connection refused"
        ));
    }
    want
}

#[test]
fn frozen_go_observations_replay_natively() {
    let mut fixtures: Vec<Value> = Vec::new();
    let mut names: Vec<PathBuf> = std::fs::read_dir(fixtures_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension().is_some_and(|e| e == "json")
                && path.file_name().is_some_and(|n| n != "provenance.json")
        })
        .collect();
    names.sort();
    for path in names {
        fixtures.push(serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap());
    }
    // MEMORY_SYNC_ONLY=<substring> narrows a debugging run to matching scenarios.
    let only = std::env::var("MEMORY_SYNC_ONLY").unwrap_or_default();
    if only.is_empty() {
        assert!(fixtures.len() > 100, "fixture set went missing");
    }
    fixtures.retain(|fixture| fixture["name"].as_str().unwrap().contains(&only));
    let scratch = tempfile::tempdir().unwrap();
    let queue = Mutex::new(fixtures.iter().collect::<Vec<_>>());
    let problems = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                loop {
                    let Some(next) = queue.lock().unwrap().pop() else {
                        return;
                    };
                    let found = run_fixture(next, scratch.path());
                    problems.lock().unwrap().extend(found);
                }
            });
        }
    });
    let problems = problems.into_inner().unwrap();
    assert!(
        problems.is_empty(),
        "{} differences:\n{}",
        problems.len(),
        problems.join("\n")
    );
}
