//! Process harness shared by the `memory serve` differential tests: isolated
//! roots, identical seeded databases, a raw HTTP client and the JWT helper.
#![allow(dead_code, clippy::pedantic)]

use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

pub const SECRET: &str = "differential-test-secret-0123456789abcdef";
pub const FIXED_TIME: &str = "2030-01-02T03:04:05Z";

/// The Go binary used ONLY to record fixtures
/// (`SYMBRAIN_MEMORY_FIXTURE_RECORD=1` plus `SYMBRAIN_MEMORY_SERVE_GO_ORACLE`).
pub fn oracle() -> Option<PathBuf> {
    std::env::var_os("SYMBRAIN_MEMORY_SERVE_GO_ORACLE").map(PathBuf::from)
}

pub fn record_mode() -> bool {
    std::env::var("SYMBRAIN_MEMORY_FIXTURE_RECORD").is_ok_and(|value| value == "1")
}

/// Directory of the frozen Go observations of this test binary.
pub fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/memory_native/memory_serve_oracle")
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn git(args: &[&str]) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_default()
}

fn provenance(name: &str, command: &str, oracle_bin: &Path) -> serde_json::Value {
    let dirty = git(&["status", "--porcelain", "--", "cmd", "internal", "go.mod", "go.sum"]);
    assert!(dirty.is_empty(), "recording needs a clean Go source tree:\n{dirty}");
    let version = Command::new("go")
        .arg("version")
        .arg(oracle_bin)
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_default();
    let seed = std::fs::read(fixture_dir().join("seed.db")).unwrap_or_default();
    serde_json::json!({
        "kind": "go-oracle-recording",
        "fixture": name,
        "command": command,
        "record_mode": "SYMBRAIN_MEMORY_FIXTURE_RECORD=1 SYMBRAIN_MEMORY_SERVE_GO_ORACLE=<go build of ./cmd/symbrain>",
        "go_source": {
            "git_head": git(&["rev-parse", "HEAD"]),
            "tree_cmd": git(&["rev-parse", "HEAD:cmd"]),
            "tree_internal": git(&["rev-parse", "HEAD:internal"]),
            "go_mod": git(&["rev-parse", "HEAD:go.mod"]),
            "go_sum": git(&["rev-parse", "HEAD:go.sum"]),
        },
        "go_version": version,
        "platform": format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH),
        "oracle_sha256": digest(&std::fs::read(oracle_bin).unwrap_or_default()),
        "seed_sha256": digest(&seed),
    })
}

/// The Go behavior of one differential case, frozen. Replay reads the
/// recording (no Go involved); with `SYMBRAIN_MEMORY_FIXTURE_RECORD=1` the Go
/// binary runs once and the recording is rewritten.
pub fn frozen<T>(name: &str, command: &str, go: impl FnOnce(&Path) -> T) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let path = fixture_dir().join(format!("{name}.json"));
    if record_mode() {
        let oracle = oracle().expect("recording needs SYMBRAIN_MEMORY_SERVE_GO_ORACLE");
        let value = go(&oracle);
        let payload = serde_json::to_value(&value).expect("payload");
        let mut provenance = provenance(name, command, &oracle);
        provenance["payload_sha256"] =
            serde_json::Value::String(digest(serde_json::to_string(&payload).expect("payload").as_bytes()));
        std::fs::create_dir_all(fixture_dir()).expect("fixture directory");
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&serde_json::json!({"provenance": provenance, "payload": payload}))
                .expect("fixture")
                + "\n",
        )
        .expect("write fixture");
        return value;
    }
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("missing Go-oracle fixture {} ({error}); record it with SYMBRAIN_MEMORY_FIXTURE_RECORD=1", path.display())
    });
    let document: serde_json::Value = serde_json::from_str(&text).expect("fixture json");
    assert_eq!(
        document["provenance"]["payload_sha256"].as_str(),
        Some(digest(serde_json::to_string(&document["payload"]).expect("payload").as_bytes()).as_str()),
        "{name}: fixture payload digest"
    );
    assert_eq!(document["provenance"]["kind"], "go-oracle-recording");
    let seed = std::fs::read(fixture_dir().join("seed.db")).unwrap_or_default();
    assert_eq!(
        document["provenance"]["seed_sha256"].as_str(),
        Some(digest(&seed).as_str()),
        "{name}: recorded against a different seed database"
    );
    serde_json::from_value(document["payload"].clone()).expect("fixture payload")
}

pub fn rust_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_symbrain"))
}

pub fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn hmac(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut block = [0_u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(block.map(|b| b ^ 0x36));
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(block.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

pub fn b64url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..=chunk.len() {
            out.push(TABLE[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

pub struct Claims<'a> {
    pub sub: &'a str,
    pub jti: &'a str,
    pub iss: &'a str,
    pub exp_offset: i64,
}

pub fn token_with(secret: &str, claims: &Claims<'_>) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let header = b64url(br#"{"alg":"HS256","typ":"JWT"}"#);
    let payload = b64url(
        format!(
            r#"{{"jti":"{}","iss":"{}","sub":"{}","iat":{},"exp":{}}}"#,
            claims.jti,
            claims.iss,
            claims.sub,
            now,
            now + claims.exp_offset
        )
        .as_bytes(),
    );
    let signature = b64url(&hmac(
        secret.as_bytes(),
        format!("{header}.{payload}").as_bytes(),
    ));
    format!("{header}.{payload}.{signature}")
}

pub fn token(sub: &str, jti: &str) -> String {
    token_with(
        SECRET,
        &Claims {
            sub,
            jti,
            iss: "symaira-memory",
            exp_offset: 3600,
        },
    )
}

/// One implementation's isolated roots.
pub struct Root {
    pub dir: tempfile::TempDir,
}

impl Root {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        for sub in ["home", "cfg", "data", "cache", "proj"] {
            std::fs::create_dir_all(dir.path().join(sub)).unwrap();
        }
        Self { dir }
    }

    pub fn path(&self, sub: &str) -> PathBuf {
        self.dir.path().join(sub)
    }

    pub fn db(&self) -> PathBuf {
        self.path("data/symbrain/memory/default.db")
    }

    pub fn command(&self, binary: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(binary);
        command
            .env_clear()
            .env("HOME", self.path("home"))
            .env("USERPROFILE", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("cfg"))
            .env("XDG_DATA_HOME", self.path("data"))
            .env("XDG_CACHE_HOME", self.path("cache"))
            .env("PATH", "/usr/bin:/bin")
            .env("TZ", "UTC")
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .env("JWT_SECRET_KEY", SECRET)
            .env("SYMBRAIN_GO_BINARY", self.path("absent-go"))
            .current_dir(self.path("proj"))
            .args(["memory"])
            .args(args);
        command
    }
}

/// A running `memory serve` process.
pub struct Server {
    child: Child,
    pub port: u16,
    pub out: PathBuf,
    pub err: PathBuf,
}

impl Server {
    pub fn start(root: &Root, binary: &Path, extra: &[&str], envs: &[(&str, &str)]) -> Self {
        let port = free_port();
        let out = root.path("stdout.txt");
        let err = root.path("stderr.txt");
        let port_text = port.to_string();
        let mut args = vec!["serve", "--port", port_text.as_str()];
        args.extend_from_slice(extra);
        let mut command = root.command(binary, &args);
        for (key, value) in envs {
            command.env(key, value);
        }
        let child = command
            .stdout(std::fs::File::create(&out).unwrap())
            .stderr(std::fs::File::create(&err).unwrap())
            .spawn()
            .unwrap();
        let mut server = Self {
            child,
            port,
            out,
            err,
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "server did not listen");
            if let Some(status) = server.child.try_wait().unwrap() {
                panic!(
                    "server exited early: {status:?}: {}",
                    std::fs::read_to_string(&server.err).unwrap_or_default()
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        server
    }

    /// SIGTERM, then the exit code and the captured output.
    pub fn terminate(mut self) -> (Option<i32>, String, String) {
        let pid = self.child.id().to_string();
        let status = Command::new("/bin/kill")
            .args(["-TERM", &pid])
            .status()
            .unwrap();
        assert!(status.success());
        let deadline = Instant::now() + Duration::from_secs(15);
        let code = loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                break status.code();
            }
            assert!(Instant::now() < deadline, "server ignored SIGTERM");
            std::thread::sleep(Duration::from_millis(20));
        };
        (
            code,
            std::fs::read_to_string(&self.out).unwrap_or_default(),
            std::fs::read_to_string(&self.err).unwrap_or_default(),
        )
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

fn dechunk(mut body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let Some(end) = body.windows(2).position(|w| w == b"\r\n") else {
            return out;
        };
        let size = usize::from_str_radix(
            std::str::from_utf8(&body[..end])
                .unwrap_or("0")
                .split(';')
                .next()
                .unwrap()
                .trim(),
            16,
        )
        .unwrap_or(0);
        body = &body[end + 2..];
        if size == 0 || body.len() < size {
            return out;
        }
        out.extend_from_slice(&body[..size]);
        body = &body[(size + 2).min(body.len())..];
    }
}

/// Sends one literal request and reads the whole reply (the request must
/// carry `Connection: close`).
pub fn raw(port: u16, request: &str) -> Response {
    // A server may answer and close before it has read a very large body; the
    // client then sees a reset instead of the reply. The request is retried a
    // few times, and the body is written while the reply is read, so the
    // outcome does not depend on scheduling under load.
    let mut bytes = Vec::new();
    for _ in 0..4 {
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut writer = stream.try_clone().unwrap();
        let payload = request.as_bytes().to_vec();
        let sender = std::thread::spawn(move || {
            let _ = writer.write_all(&payload);
        });
        let mut reader = stream;
        bytes.clear();
        let _ = reader.read_to_end(&mut bytes);
        let _ = sender.join();
        if !bytes.is_empty() {
            break;
        }
    }
    let split = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or_else(|| panic!("no response head: {:?}", String::from_utf8_lossy(&bytes)));
    let head = String::from_utf8_lossy(&bytes[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_owned()))
        .collect::<Vec<_>>();
    let chunked = headers
        .iter()
        .any(|(k, v)| k == "transfer-encoding" && v == "chunked");
    let raw_body = &bytes[split + 4..];
    Response {
        status,
        headers,
        body: if chunked {
            dechunk(raw_body)
        } else {
            raw_body.to_vec()
        },
    }
}

pub fn request(
    port: u16,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> Response {
    let mut text = format!("{method} {target} HTTP/1.1\r\n");
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
        text.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
    }
    for (key, value) in headers {
        text.push_str(&format!("{key}: {value}\r\n"));
    }
    if let Some(body) = body {
        text.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    text.push_str("Connection: close\r\n\r\n");
    if let Some(body) = body {
        text.push_str(body);
    }
    raw(port, &text)
}

/// Creates the deterministic seed database with the native store. Both
/// implementations then open byte-identical copies.
pub fn seed(path: &Path) -> Vec<String> {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let store = symbrain_memory::Store::open(path).unwrap();
    let mut ids = Vec::new();
    for (content, scope) in [
        ("alpha rocket notes", "global"),
        ("beta garden journal", "project"),
        ("gamma harbor log", "global"),
        ("delta mountain diary", "agent"),
    ] {
        let memory = store
            .set(content, scope, "note", serde_json::Map::new(), false)
            .unwrap();
        ids.push(memory.id);
    }
    let removed = store
        .set(
            "epsilon ephemeral entry",
            "global",
            "note",
            serde_json::Map::new(),
            false,
        )
        .unwrap();
    assert!(store.delete(&removed.id).unwrap());
    ids.push(removed.id);
    drop(store);
    let conn = Connection::open(path).unwrap();
    conn.execute(
        "UPDATE memories SET content='contact alice@example.com about sk-abcdefghijklmnopqrstuv', metadata='{\"note\":\"call +49 170 1234567\",\"sensitivity\":\"confidential\",\"sharing_level\":\"team\"}' WHERE id=?",
        [&ids[0]],
    )
    .unwrap();
    conn.execute(
        "UPDATE memories SET metadata='{\"allowed_clients\":\"agent-one, agent-two\",\"denied_clients\":\"blocked\",\"sensitivity\":\"public\",\"sharing_level\":\"org\"}' WHERE id=?",
        [&ids[2]],
    )
    .unwrap();
    for (id, content, scope) in [
        ("rule-1", "always answer in German", "global"),
        ("rule-2", "project rule: tabs", "project"),
    ] {
        conn.execute(
            "INSERT INTO rules(id,content,scope,metadata,created_at,updated_at,created_by,updated_by) VALUES(?,?,?,?,?,?,?,?)",
            rusqlite::params![id, content, scope, "{\"k\":\"v\"}", "2026-01-02 03:04:05.123456 +0000 UTC", "2026-01-03 03:04:05 +0000 UTC", "seed", ""],
        )
        .unwrap();
    }
    for (id, name, aliases) in [
        ("ent-1", "Alice", "[\"Al\",\"A.\"]"),
        ("ent-2", "Zeta Project", "[]"),
    ] {
        conn.execute(
            "INSERT INTO entities(id,name,type,aliases,description,created_by,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?)",
            rusqlite::params![id, name, "person", aliases, "desc <b>&", if id == "ent-1" { "seed" } else { "" }, "2026-01-02 03:04:05 +0000 UTC", "2026-01-02 03:04:05.5 +0000 UTC"],
        )
        .unwrap();
    }
    for (id, name, role) in [("p-1", "reader", "read"), ("p-2", "writer", "readwrite")] {
        conn.execute(
            "INSERT INTO profiles(id,name,type,role,description,metadata,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?)",
            rusqlite::params![id, name, "agent", role, "", "{}", "2026-01-02 03:04:05 +0000 UTC", "2026-01-02 03:04:05 +0000 UTC"],
        )
        .unwrap();
    }
    conn.execute("PRAGMA wal_checkpoint(TRUNCATE)", []).ok();
    ids
}

pub fn install_seed(root: &Root, seed_db: &Path) {
    let target = root.db();
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::copy(seed_db, &target).unwrap();
}
