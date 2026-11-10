//! Differential harness for the native `symbrain memory` verbs (#758).
//!
//! The Go oracle is built from this worktree (or taken from
//! `SYMBRAIN_MEMORY_ORACLE`), both implementations run the identical argv in
//! identical, isolated HOME/XDG roots against identical database fixtures that
//! the Go oracle itself produced, and every observable is compared: exit code,
//! stdout and stderr bytes, the whole database state and a manifest of the
//! created files. The Rust process never has a Go fallback available.

#![allow(dead_code, clippy::pedantic)]

#[path = "../../../test-support/fs_walk.rs"]
mod fs_walk;

use rusqlite::{Connection, OpenFlags, types::ValueRef};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// Which implementation executes a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    Go,
    Rust,
}

/// One disposable HOME/XDG/cwd tree.
pub struct Sandbox {
    _guard: tempfile::TempDir,
    pub root: PathBuf,
}

impl Sandbox {
    pub fn new() -> Self {
        let guard = tempfile::Builder::new()
            .prefix("memnat-")
            .tempdir()
            .expect("sandbox directory");
        let root = guard.path().canonicalize().expect("canonical sandbox");
        for dir in [
            "home",
            "work",
            "xdg/config",
            "xdg/data",
            "xdg/cache",
            "xdg/state",
        ] {
            std::fs::create_dir_all(root.join(dir)).expect("sandbox layout");
        }
        Self {
            _guard: guard,
            root,
        }
    }

    pub fn work(&self) -> PathBuf {
        self.root.join("work")
    }

    pub fn db(&self) -> PathBuf {
        self.root.join("memory.db")
    }

    /// Writes the global `symmemory` configuration.
    pub fn write_config(&self, toml: &str) {
        let dir = self.root.join("xdg/config/symmemory");
        std::fs::create_dir_all(&dir).expect("config directory");
        std::fs::write(dir.join("config.toml"), toml).expect("config file");
    }
}

/// Captured process result.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn git_key(root: &Path) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "rev-parse",
            "HEAD:cmd",
            "HEAD:internal",
            "HEAD:go.mod",
            "HEAD:go.sum",
        ])
        .output();
    match output {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout).into(),
        _ => String::new(),
    }
}

/// The Go oracle binary: `SYMBRAIN_MEMORY_ORACLE`, else a build of this
/// worktree under `target/memory-native-oracle` (rebuilt when the Go sources
/// change).
pub fn oracle() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        if let Some(path) = std::env::var_os("SYMBRAIN_MEMORY_ORACLE") {
            let path = PathBuf::from(path);
            assert!(path.is_file(), "SYMBRAIN_MEMORY_ORACLE is not a file");
            return path;
        }
        let root = repo_root();
        let dir = root.join("target/memory-native-oracle");
        std::fs::create_dir_all(&dir).expect("oracle directory");
        let bin = dir.join("symbrain");
        let key_file = dir.join("symbrain.key");
        let key = git_key(&root);
        let current = std::fs::read_to_string(&key_file).unwrap_or_default();
        if bin.is_file() && !key.is_empty() && key == current {
            return bin;
        }
        let status = Command::new("go")
            .args(["build", "-o"])
            .arg(&bin)
            .arg("./cmd/symbrain")
            .current_dir(&root)
            .env("CGO_ENABLED", "0")
            .status()
            .expect("go build of the oracle");
        assert!(status.success(), "building the Go oracle failed");
        std::fs::write(&key_file, key).expect("oracle key");
        bin
    })
}

fn base_command(who: Who, sb: &Sandbox, rust_bin: &str) -> Command {
    let program: PathBuf = match who {
        Who::Go => oracle().to_path_buf(),
        Who::Rust => PathBuf::from(rust_bin),
    };
    let mut command = Command::new(program);
    command
        .env_clear()
        .env("HOME", sb.root.join("home"))
        .env("XDG_CONFIG_HOME", sb.root.join("xdg/config"))
        .env("XDG_DATA_HOME", sb.root.join("xdg/data"))
        .env("XDG_CACHE_HOME", sb.root.join("xdg/cache"))
        .env("XDG_STATE_HOME", sb.root.join("xdg/state"))
        .env("PATH", "")
        .env("TZ", "UTC")
        .env("LC_ALL", "C")
        .current_dir(sb.work())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if who == Who::Rust {
        // The native implementation must not need the shipped binary.
        command.env("SYMBRAIN_GO_BINARY", sb.root.join("absent-go-fallback"));
        // Under coverage the instrumented child would otherwise write its
        // .profraw into the work directory and break the created-files check.
        command.envs(std::env::var_os("LLVM_PROFILE_FILE").map(|path| ("LLVM_PROFILE_FILE", path)));
    }
    command
}

/// Runs one invocation of `symbrain memory <args>`.
pub fn run(
    who: Who,
    sb: &Sandbox,
    rust_bin: &str,
    args: &[OsString],
    env: &[(&str, OsString)],
) -> Outcome {
    let mut command = base_command(who, sb, rust_bin);
    command.arg("memory").args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command.output().expect("process output");
    Outcome {
        code: output.status.code(),
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

/// Convenience for string arguments.
pub fn argv(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

fn slog_prefix() -> &'static regex::bytes::Regex {
    static RE: OnceLock<regex::bytes::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::bytes::Regex::new(r"(?m)^\d{4}/\d{2}/\d{2} \d{2}:\d{2}:\d{2} ").expect("regex")
    })
}

fn uuid_re() -> &'static regex::bytes::Regex {
    static RE: OnceLock<regex::bytes::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::bytes::Regex::new(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
            .expect("regex")
    })
}

/// Output with run-specific bytes replaced: the sandbox root, the log
/// timestamp and (optionally) generated identifiers.
pub fn normalize_output(raw: &[u8], root: &Path, ids: &mut IdMap) -> Vec<u8> {
    let root_text = root.to_string_lossy().into_owned();
    let mut bytes = replace_bytes(raw, root_text.as_bytes(), b"<root>");
    bytes = slog_prefix()
        .replace_all(&bytes, &b"<time> "[..])
        .into_owned();
    let bytes = uuid_re()
        .replace_all(&bytes, |caps: &regex::bytes::Captures<'_>| {
            let id = String::from_utf8_lossy(&caps[0]).into_owned();
            ids.label(&id).into_bytes()
        })
        .into_owned();
    // Retrieval scores blend in `time.Since(updated_at)`, so their digits
    // depend on when the process ran; the ranking order stays compared.
    let bytes = score_json()
        .replace_all(&bytes, &b"\"similarity_score\":<score>"[..])
        .into_owned();
    let bytes = score_table()
        .replace_all(&bytes, &b"<score>\t"[..])
        .into_owned();
    // Access tracking stamps the time of the search itself.
    access_stamp()
        .replace_all(&bytes, &b"\"${1}\":\"<ts>\""[..])
        .into_owned()
}

fn access_stamp() -> &'static regex::bytes::Regex {
    static RE: OnceLock<regex::bytes::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::bytes::Regex::new(r#""(last_access|prev_access)":"[^"]*""#).expect("regex")
    })
}

fn score_json() -> &'static regex::bytes::Regex {
    static RE: OnceLock<regex::bytes::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::bytes::Regex::new(r#""similarity_score":[0-9eE+.\-]+"#).expect("regex")
    })
}

fn score_table() -> &'static regex::bytes::Regex {
    static RE: OnceLock<regex::bytes::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::bytes::Regex::new(r"(?m)^[0-9]\.[0-9]{6}\t").expect("regex"))
}

fn replace_bytes(haystack: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    if needle.is_empty() {
        return haystack.to_vec();
    }
    let mut out = Vec::with_capacity(haystack.len());
    let mut index = 0;
    while index < haystack.len() {
        if haystack[index..].starts_with(needle) {
            out.extend_from_slice(with);
            index += needle.len();
        } else {
            out.push(haystack[index]);
            index += 1;
        }
    }
    out
}

/// Stable labels for generated identifiers, in order of first appearance.
#[derive(Default)]
pub struct IdMap {
    known: BTreeSet<String>,
    labels: BTreeMap<String, String>,
}

impl IdMap {
    pub fn with_known(known: BTreeSet<String>) -> Self {
        Self {
            known,
            labels: BTreeMap::new(),
        }
    }

    pub fn label(&mut self, id: &str) -> String {
        if self.known.contains(id) {
            return id.to_owned();
        }
        let next = self.labels.len() + 1;
        self.labels
            .entry(id.to_owned())
            .or_insert_with(|| format!("<id{next}>"))
            .clone()
    }
}

const DUMP_TABLES: &[&str] = &[
    "memories",
    "entities",
    "entities_aliases",
    "memory_entities",
    "audit_log",
    "memory_evidence",
    "rules",
    "query_log",
    "sync_oplog",
];

fn render(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Null => "NULL".into(),
        ValueRef::Integer(v) => v.to_string(),
        ValueRef::Real(v) => format!("{v:?}"),
        ValueRef::Text(t) => format!("{:?}", String::from_utf8_lossy(t)),
        ValueRef::Blob(b) => {
            let hex: String = b.iter().map(|byte| format!("{byte:02x}")).collect();
            format!("x'{hex}'")
        }
    }
}

fn text_cells(path: &Path) -> Vec<(String, Vec<String>)> {
    let Ok(conn) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return Vec::new();
    };
    let mut tables = Vec::new();
    for table in DUMP_TABLES {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name=?",
                [table],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if exists == 0 {
            continue;
        }
        let mut columns: Vec<String> = conn
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .and_then(|mut s| {
                s.query_map([], |r| r.get::<_, String>(0))
                    .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            })
            .unwrap_or_default();
        columns.sort();
        let sql = format!(
            "SELECT {} FROM {table} ORDER BY rowid",
            columns
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(",")
        );
        let mut statement = conn.prepare(&sql).expect("dump statement");
        let mut rows = statement.query([]).expect("dump rows");
        let mut rendered = Vec::new();
        while let Some(row) = rows.next().expect("dump row") {
            let cells: Vec<String> = columns
                .iter()
                .enumerate()
                .map(|(i, name)| format!("{name}={}", render(row.get_ref(i).expect("cell"))))
                .collect();
            rendered.push(cells.join("|"));
        }
        tables.push(((*table).to_owned(), rendered));
    }
    tables
}

fn ts_re() -> &'static [(regex::Regex, &'static str)] {
    static RE: OnceLock<Vec<(regex::Regex, &'static str)>> = OnceLock::new();
    RE.get_or_init(|| {
        vec![
            (
                regex::Regex::new(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(\.\d{1,9})? \+0000 UTC")
                    .expect("regex"),
                "<ts>",
            ),
            (
                regex::Regex::new(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{1,9})?Z")
                    .expect("regex"),
                "<isots>",
            ),
            (
                regex::Regex::new(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}").expect("regex"),
                "<plainTs>",
            ),
        ]
    })
}

/// Identifiers present in a database (to keep seeded ids literal).
pub fn known_ids(path: &Path) -> BTreeSet<String> {
    // Inspect a copy: opening a WAL database creates side files next to it,
    // and the fixture directory must stay untouched.
    let scratch = tempfile::tempdir().expect("scratch directory");
    let copy = scratch.path().join("seed.db");
    std::fs::copy(path, &copy).expect("seed copy");
    let path = copy.as_path();
    let mut ids = BTreeSet::new();
    let re = regex::Regex::new(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
        .expect("regex");
    for (_, rows) in text_cells(path) {
        for row in rows {
            for found in re.find_iter(&row) {
                ids.insert(found.as_str().to_owned());
            }
        }
    }
    ids
}

/// Whole-database snapshot with generated ids and timestamps normalized.
pub fn snapshot(path: &Path, ids: &mut IdMap) -> Vec<String> {
    let uuid = regex::Regex::new(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
        .expect("regex");
    let mut out = Vec::new();
    for (table, rows) in text_cells(path) {
        out.push(format!("== {table} ({} rows)", rows.len()));
        for row in rows {
            let mut text = uuid
                .replace_all(&row, |caps: &regex::Captures<'_>| ids.label(&caps[0]))
                .into_owned();
            for (re, label) in ts_re() {
                text = re.replace_all(&text, *label).into_owned();
            }
            out.push(text);
        }
    }
    out
}

/// Recursive manifest of a sandbox: relative path, type and permission bits.
pub fn manifest(root: &Path) -> Vec<String> {
    use std::os::unix::fs::PermissionsExt;
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .map(|rd| rd.filter_map(Result::ok).collect())
            .unwrap_or_default();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            // WAL side files appear and vanish with connection state.
            if rel.ends_with("-wal") || rel.ends_with("-shm") || rel.ends_with("-journal") {
                continue;
            }
            let Some(meta) = fs_walk::skip_vanished(std::fs::symlink_metadata(&path), &path) else {
                continue;
            };
            let kind = if meta.is_dir() { "dir" } else { "file" };
            out.push(format!(
                "{kind} {rel} {:o}",
                meta.permissions().mode() & 0o7777
            ));
            if meta.is_dir() {
                walk(root, &path, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}

/// A loopback HTTP server with scripted replies; records every request.
pub struct FakeServer {
    pub url: String,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    stop: Arc<AtomicBool>,
}

#[derive(Clone, Debug)]
pub struct RecordedRequest {
    pub request_line: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

pub struct FakeReply {
    pub status: u16,
    pub body: Vec<u8>,
    pub content_type: &'static str,
}

type Handler = dyn Fn(&RecordedRequest) -> FakeReply + Send + Sync;

impl FakeServer {
    pub fn start(handler: Arc<Handler>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        listener.set_nonblocking(true).expect("nonblocking");
        let port = listener.local_addr().expect("addr").port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let log = Arc::clone(&requests);
        let flag = Arc::clone(&stop);
        std::thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let handler = Arc::clone(&handler);
                        let log = Arc::clone(&log);
                        std::thread::spawn(move || serve(stream, &*handler, &log));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Self {
            url: format!("http://127.0.0.1:{port}"),
            requests,
            stop,
        }
    }

    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().expect("log").clone()
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn serve(stream: TcpStream, handler: &Handler, log: &Mutex<Vec<RecordedRequest>>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut writer = match stream.try_clone() {
        Ok(writer) => writer,
        Err(_) => return,
    };
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return;
    }
    let mut headers = BTreeMap::new();
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let line = line.trim_end_matches(['\r', '\n']).to_owned();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_owned();
            if name == "content-length" {
                length = value.parse().unwrap_or(0);
            }
            headers.insert(name, value);
        }
    }
    let mut body = vec![0_u8; length];
    let _ = reader.read_exact(&mut body);
    let request = RecordedRequest {
        request_line: request_line.trim_end().to_owned(),
        headers,
        body,
    };
    log.lock().expect("log").push(request.clone());
    let reply = handler(&request);
    let head = format!(
        "HTTP/1.1 {} X\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reply.status,
        reply.content_type,
        reply.body.len()
    );
    let _ = writer.write_all(head.as_bytes());
    let _ = writer.write_all(&reply.body);
}

/// A fixed 768-dimensional embedding reply.
pub fn embedding_reply(seed: usize) -> FakeReply {
    let vector: Vec<f32> = (0..768)
        .map(|i| if i % 97 == seed % 97 { 1.0 } else { 0.0 })
        .collect();
    FakeReply {
        status: 200,
        body: serde_json::json!({"data": [{"index": 0, "embedding": vector}]})
            .to_string()
            .into_bytes(),
        content_type: "application/json",
    }
}

/// A differential scenario.
pub struct Scenario<'a> {
    pub name: &'a str,
    pub rust_bin: &'a str,
    /// Global configuration; `{EMBED}` / `{LLM}` expand to server URLs.
    pub config: String,
    /// Steps run by the Go oracle in a scratch sandbox to build the fixture.
    pub seed: Vec<Vec<OsString>>,
    /// SQL applied to the fixture database before the steps (fault
    /// injection); recorded in the fixture and applied to both sides.
    pub mutate_sql: Vec<String>,
    /// Scenarios with the same key and seed steps share one recorded seed
    /// database (`seed-<key>.db`).
    pub seed_key: Option<&'a str>,
    /// Steps compared between the implementations. `{DB}` is the database.
    pub steps: Vec<Vec<OsString>>,
    pub env: Vec<(&'static str, OsString)>,
    /// When true no fixture database is created before the steps.
    pub no_db: bool,
    /// Embedding endpoint behaviour; `None` leaves it unreachable.
    pub embed: Option<Arc<Handler>>,
    pub llm: Option<Arc<Handler>>,
    /// Compare stderr only after dropping log lines (nondeterministic order).
    pub stderr_as_set: bool,
}

impl<'a> Scenario<'a> {
    pub fn new(name: &'a str, rust_bin: &'a str) -> Self {
        Self {
            name,
            rust_bin,
            config: String::new(),
            seed: Vec::new(),
            mutate_sql: Vec::new(),
            seed_key: None,
            steps: Vec::new(),
            env: Vec::new(),
            no_db: false,
            embed: None,
            llm: None,
            stderr_as_set: false,
        }
    }
}

/// Result of a scenario for extra assertions.
pub struct Compared {
    pub go: Vec<Outcome>,
    pub rust: Vec<Outcome>,
    pub go_requests: (Vec<RecordedRequest>, Vec<RecordedRequest>),
    pub rust_requests: (Vec<RecordedRequest>, Vec<RecordedRequest>),
    /// Normalized database snapshot after the steps (identical for both).
    pub db: Vec<String>,
}

fn first_id(db: &Path) -> OsString {
    Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|conn| {
            conn.query_row(
                "SELECT id FROM memories ORDER BY rowid LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
        })
        .map(OsString::from)
        .unwrap_or_default()
}

/// `{DB}` is the sandbox database, `{FIRST_ID}` the oldest memory id in it.
fn expand(args: &[OsString], db: &Path) -> Vec<OsString> {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    args.iter()
        .map(|arg| {
            if arg == OsStr::new("{DB}") {
                db.as_os_str().to_owned()
            } else if arg.as_bytes().windows(10).any(|w| w == b"{FIRST_ID}") {
                let id = first_id(db);
                OsString::from_vec(replace_bytes(arg.as_bytes(), b"{FIRST_ID}", id.as_bytes()))
            } else {
                arg.clone()
            }
        })
        .collect()
}

fn expand_config(
    scenario: &Scenario<'_>,
    embed: Option<&FakeServer>,
    llm: Option<&FakeServer>,
) -> String {
    scenario
        .config
        .replace(
            "{EMBED}",
            &embed.map_or_else(
                || "owned-invalid-endpoint".to_owned(),
                |s| format!("{}/api/embeddings", s.url),
            ),
        )
        .replace(
            "{LLM}",
            &llm.map_or_else(|| "http://127.0.0.1:1".to_owned(), |s| s.url.clone()),
        )
}

/// Everything one implementation produced for a scenario.
struct Side {
    outs: Vec<Outcome>,
    stdouts: Vec<Vec<u8>>,
    stderrs: Vec<Vec<u8>>,
    db: Vec<String>,
    tree: Vec<String>,
    requests: (Vec<RecordedRequest>, Vec<RecordedRequest>),
}

fn run_side(
    who: Who,
    scenario: &Scenario<'_>,
    seed_db: Option<&Path>,
    known: &BTreeSet<String>,
) -> Side {
    let sb = Sandbox::new();
    let embed_server = scenario
        .embed
        .as_ref()
        .map(|h| FakeServer::start(Arc::clone(h)));
    let llm_server = scenario
        .llm
        .as_ref()
        .map(|h| FakeServer::start(Arc::clone(h)));
    sb.write_config(&expand_config(
        scenario,
        embed_server.as_ref(),
        llm_server.as_ref(),
    ));
    if let Some(seed) = seed_db {
        std::fs::copy(seed, sb.db()).expect("fixture copy");
        if !scenario.mutate_sql.is_empty() {
            let conn = Connection::open(sb.db()).expect("mutate connection");
            // Triggers (sync log, JSON checks) would reject or log the
            // fault injection itself; they are restored right after.
            let triggers: Vec<(String, String)> = conn
                .prepare("SELECT name, sql FROM sqlite_schema WHERE type='trigger'")
                .and_then(|mut statement| {
                    statement
                        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                        .and_then(|rows| rows.collect())
                })
                .expect("trigger list");
            for (name, _) in &triggers {
                conn.execute_batch(&format!("DROP TRIGGER \"{name}\""))
                    .expect("trigger drop");
            }
            for statement in &scenario.mutate_sql {
                conn.execute_batch(statement).expect("fixture mutation");
            }
            for (_, sql) in &triggers {
                conn.execute_batch(sql).expect("trigger restore");
            }
        }
    }
    let mut ids = IdMap::with_known(known.clone());
    let mut outs = Vec::new();
    for step in &scenario.steps {
        outs.push(run(
            who,
            &sb,
            scenario.rust_bin,
            &expand(step, &sb.db()),
            &scenario.env,
        ));
    }
    let mut stdouts = Vec::new();
    let mut stderrs = Vec::new();
    for out in &outs {
        stdouts.push(normalize_output(&out.stdout, &sb.root, &mut ids));
        stderrs.push(normalize_output(&out.stderr, &sb.root, &mut ids));
    }
    let db = snapshot(&sb.db(), &mut ids);
    let tree = manifest(&sb.root);
    let requests = (
        embed_server
            .as_ref()
            .map(FakeServer::requests)
            .unwrap_or_default(),
        llm_server
            .as_ref()
            .map(FakeServer::requests)
            .unwrap_or_default(),
    );
    Side {
        outs,
        stdouts,
        stderrs,
        db,
        tree,
        requests,
    }
}

// --- Fixtures ------------------------------------------------------------

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn encode(bytes: &[u8]) -> serde_json::Value {
    match std::str::from_utf8(bytes) {
        Ok(text) => serde_json::Value::String(text.to_owned()),
        Err(_) => {
            serde_json::json!({"hex": bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()})
        }
    }
}

fn decode(value: &serde_json::Value) -> Vec<u8> {
    match value {
        serde_json::Value::String(text) => text.as_bytes().to_vec(),
        other => {
            let hex = other["hex"].as_str().expect("hex-encoded bytes");
            (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("hex"))
                .collect()
        }
    }
}

fn request_json(request: &RecordedRequest) -> serde_json::Value {
    let headers: BTreeMap<&String, &String> = request
        .headers
        .iter()
        .filter(|(name, _)| !matches!(name.as_str(), "host" | "content-length"))
        .collect();
    serde_json::json!({
        "request_line": request.request_line,
        "headers": headers,
        "body": encode(&request.body),
    })
}

fn request_from(value: &serde_json::Value) -> RecordedRequest {
    RecordedRequest {
        request_line: value["request_line"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        headers: value["headers"]
            .as_object()
            .map(|map| {
                map.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                    .collect()
            })
            .unwrap_or_default(),
        body: decode(&value["body"]),
    }
}

fn fixture_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("test executable");
    let stem = exe.file_stem().and_then(|s| s.to_str()).unwrap_or("memory");
    // `memory_native_read-1a2b3c` -> `memory_native_read`.
    let stem = stem.rsplit_once('-').map_or(stem, |(name, _)| name);
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/memory_native")
        .join(stem)
}

fn record_mode() -> bool {
    std::env::var("SYMBRAIN_MEMORY_FIXTURE_RECORD").is_ok_and(|value| value == "1")
}

fn command_output(program: &str, args: &[&str], dir: &Path) -> String {
    Command::new(program)
        .args(args)
        .current_dir(dir)
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_default()
}

/// Provenance of a recording: the Go sources and toolchain that produced it.
fn provenance(oracle_bin: &Path) -> serde_json::Value {
    let root = repo_root();
    let dirty = command_output(
        "git",
        &[
            "status",
            "--porcelain",
            "--",
            "cmd",
            "internal",
            "go.mod",
            "go.sum",
        ],
        &root,
    );
    assert!(
        dirty.is_empty(),
        "recording needs a clean Go source tree, found:\n{dirty}"
    );
    let rev = |spec: &str| command_output("git", &["rev-parse", spec], &root);
    let version = command_output("go", &["version", &oracle_bin.to_string_lossy()], &root);
    let bytes = std::fs::read(oracle_bin).expect("oracle binary");
    serde_json::json!({
        "kind": "go-oracle-recording",
        "record_mode": "SYMBRAIN_MEMORY_FIXTURE_RECORD=1",
        "go_source": {
            "git_head": rev("HEAD"),
            "tree_cmd": rev("HEAD:cmd"),
            "tree_internal": rev("HEAD:internal"),
            "go_mod": rev("HEAD:go.mod"),
            "go_sum": rev("HEAD:go.sum"),
        },
        "go_version": version,
        "platform": format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH),
        "oracle_sha256": sha256_hex(&bytes),
    })
}

fn payload_hash(body: &serde_json::Value) -> String {
    sha256_hex(serde_json::to_string(body).expect("payload").as_bytes())
}

fn side_payload(
    scenario: &Scenario<'_>,
    side: &Side,
    seed_file: &str,
    seed_sha: &str,
) -> serde_json::Value {
    let steps: Vec<serde_json::Value> = side
        .outs
        .iter()
        .enumerate()
        .map(|(index, out)| {
            serde_json::json!({
                "argv": scenario.steps[index].iter().map(|arg| {
                    use std::os::unix::ffi::OsStrExt;
                    encode(arg.as_bytes())
                }).collect::<Vec<_>>(),
                "code": out.code,
                "stdout": encode(&side.stdouts[index]),
                "stderr": encode(&side.stderrs[index]),
            })
        })
        .collect();
    serde_json::json!({
        "scenario": scenario.name,
        "config": scenario.config,
        "seed_steps": scenario.seed.iter().map(|step| step.iter().map(|arg| {
            use std::os::unix::ffi::OsStrExt;
            encode(arg.as_bytes())
        }).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "seed_file": seed_file,
        "seed_sha256": seed_sha,
        "mutate_sql": scenario.mutate_sql,
        "steps": steps,
        "db": side.db,
        "tree": side.tree,
        "requests": {
            "embed": side.requests.0.iter().map(request_json).collect::<Vec<_>>(),
            "llm": side.requests.1.iter().map(request_json).collect::<Vec<_>>(),
        },
    })
}

/// Loads and verifies a recorded fixture; a missing or altered fixture fails.
pub fn load_fixture(dir: &Path, name: &str) -> serde_json::Value {
    let path = dir.join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "missing Go-oracle fixture {} ({error}); record it with SYMBRAIN_MEMORY_FIXTURE_RECORD=1",
            path.display()
        )
    });
    let document: serde_json::Value = serde_json::from_str(&text).expect("fixture json");
    verify_fixture(&document, dir, name);
    document
}

/// Checks the payload digest and the seed database digest of a fixture.
pub fn verify_fixture(document: &serde_json::Value, dir: &Path, name: &str) {
    let payload = &document["payload"];
    assert_eq!(
        document["provenance"]["payload_sha256"].as_str(),
        Some(payload_hash(payload).as_str()),
        "{name}: fixture payload digest mismatch"
    );
    assert_eq!(
        document["provenance"]["kind"], "go-oracle-recording",
        "{name}: not a recording"
    );
    for field in ["go_version", "platform", "oracle_sha256"] {
        assert!(
            document["provenance"][field]
                .as_str()
                .is_some_and(|value| !value.is_empty()),
            "{name}: provenance lacks {field}"
        );
    }
    assert!(
        document["provenance"]["go_source"]["tree_internal"]
            .as_str()
            .is_some_and(|value| value.len() == 40),
        "{name}: provenance lacks the Go source revision"
    );
    let seed_sha = payload["seed_sha256"].as_str().unwrap_or_default();
    if !seed_sha.is_empty() {
        let file = payload["seed_file"].as_str().unwrap_or_default();
        let seed = std::fs::read(dir.join(file))
            .unwrap_or_else(|error| panic!("{name}: missing seed database {file} ({error})"));
        assert_eq!(sha256_hex(&seed), seed_sha, "{name}: seed database digest");
    }
}

/// Runs a scenario against the recorded Go behavior (default) or, with
/// `SYMBRAIN_MEMORY_FIXTURE_RECORD=1`, records it from the Go oracle first.
pub fn compare(scenario: &Scenario<'_>) -> Compared {
    let dir = fixture_dir();
    if record_mode() {
        record(scenario, &dir);
    }
    replay(scenario, &dir)
}

fn seed_file_name(scenario: &Scenario<'_>) -> String {
    scenario.seed_key.map_or_else(
        || format!("{}.seed.db", scenario.name),
        |key| format!("seed-{key}.db"),
    )
}

fn build_seed(scenario: &Scenario<'_>, target: &Path) {
    let seed = Sandbox::new();
    let embed_server = scenario
        .embed
        .as_ref()
        .map(|h| FakeServer::start(Arc::clone(h)));
    seed.write_config(&expand_config(scenario, embed_server.as_ref(), None));
    // Always create the schema through the oracle.
    let created = run(
        Who::Go,
        &seed,
        "",
        &argv(&["list", "--db", seed.db().to_str().unwrap()]),
        &[],
    );
    assert_eq!(
        created.code,
        Some(0),
        "fixture creation: {:?}",
        String::from_utf8_lossy(&created.stderr)
    );
    for step in &scenario.seed {
        let out = run(Who::Go, &seed, "", &expand(step, &seed.db()), &scenario.env);
        assert_eq!(
            out.code,
            Some(0),
            "{}: seed step {:?} failed: {}",
            scenario.name,
            step,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    // Checkpoint into the main file so the copy is complete.
    {
        let conn = Connection::open(seed.db()).expect("checkpoint connection");
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }
    std::fs::copy(seed.db(), target).expect("seed copy");
}

fn record(scenario: &Scenario<'_>, dir: &Path) {
    // Scenarios of different tests share seeds; build each one only once.
    static SEEDS: Mutex<()> = Mutex::new(());
    let _guard = SEEDS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    std::fs::create_dir_all(dir).expect("fixture directory");
    let seed_name = seed_file_name(scenario);
    let seed_path = dir.join(&seed_name);
    let mut seed_sha = String::new();
    if scenario.no_db {
        let _ = std::fs::remove_file(&seed_path);
    } else {
        // A shared seed is built once; delete the file to rebuild it.
        if scenario.seed_key.is_none() || !seed_path.is_file() {
            build_seed(scenario, &seed_path);
        }
        seed_sha = sha256_hex(&std::fs::read(&seed_path).expect("seed"));
    }
    let seed_db = (!scenario.no_db).then_some(seed_path.as_path());
    let known = seed_db.map(known_ids).unwrap_or_default();
    let go = run_side(Who::Go, scenario, seed_db, &known);
    let rust = run_side(Who::Rust, scenario, seed_db, &known);
    assert_sides(scenario, &go, &rust);
    let payload = side_payload(
        scenario,
        &go,
        if scenario.no_db { "" } else { &seed_name },
        &seed_sha,
    );
    let mut provenance = provenance(oracle());
    provenance["payload_sha256"] = serde_json::Value::String(payload_hash(&payload));
    let document = serde_json::json!({"provenance": provenance, "payload": payload});
    std::fs::write(
        dir.join(format!("{}.json", scenario.name)),
        serde_json::to_string_pretty(&document).expect("fixture") + "\n",
    )
    .expect("write fixture");
}

/// Fails with the first differing offset and the bytes around it.
fn assert_bytes(label: &str, want: &[u8], got: &[u8]) {
    if want == got {
        return;
    }
    let at = want
        .iter()
        .zip(got)
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| want.len().min(got.len()));
    let window = |bytes: &[u8]| {
        let start = at.saturating_sub(120).min(bytes.len());
        let end = (at + 160).min(bytes.len());
        String::from_utf8_lossy(&bytes[start..end]).into_owned()
    };
    panic!(
        "{label}: bytes differ at offset {at} (go {} bytes, rust {} bytes)\n go:   {:?}\n rust: {:?}",
        want.len(),
        got.len(),
        window(want),
        window(got)
    );
}

fn assert_sides(scenario: &Scenario<'_>, go: &Side, rust: &Side) {
    let name = scenario.name;
    assert_eq!(go.outs.len(), rust.outs.len(), "{name}: step count");
    for (index, (g, r)) in go.outs.iter().zip(&rust.outs).enumerate() {
        assert_bytes(
            &format!("{name} step {index}: stdout"),
            &go.stdouts[index],
            &rust.stdouts[index],
        );
        if scenario.stderr_as_set {
            let mut a: Vec<&[u8]> = go.stderrs[index].split(|b| *b == b'\n').collect();
            let mut b: Vec<&[u8]> = rust.stderrs[index].split(|b| *b == b'\n').collect();
            a.sort_unstable();
            b.sort_unstable();
            assert_eq!(a, b, "{name} step {index}: stderr lines");
        } else {
            assert_bytes(
                &format!("{name} step {index}: stderr"),
                &go.stderrs[index],
                &rust.stderrs[index],
            );
        }
        assert_eq!(g.code, r.code, "{name} step {index}: exit status");
    }
    assert_eq!(go.db, rust.db, "{name}: database state");
    assert_eq!(go.tree, rust.tree, "{name}: created files");
}

/// Replays the recorded Go behavior against the Rust implementation.
fn replay(scenario: &Scenario<'_>, dir: &Path) -> Compared {
    let document = load_fixture(dir, scenario.name);
    let payload = &document["payload"];
    assert_eq!(
        payload["mutate_sql"],
        serde_json::json!(scenario.mutate_sql),
        "{}: the scenario's mutations differ from the recording",
        scenario.name
    );
    let seed_path = dir.join(payload["seed_file"].as_str().unwrap_or_default());
    let seed_db = (!scenario.no_db).then_some(seed_path.as_path());
    let known = seed_db.map(known_ids).unwrap_or_default();
    let rust = run_side(Who::Rust, scenario, seed_db, &known);

    let steps = payload["steps"].as_array().expect("recorded steps");
    assert_eq!(
        steps.len(),
        rust.outs.len(),
        "{}: step count",
        scenario.name
    );
    let mut go_outs = Vec::new();
    let go_stdouts: Vec<Vec<u8>> = steps.iter().map(|s| decode(&s["stdout"])).collect();
    let go_stderrs: Vec<Vec<u8>> = steps.iter().map(|s| decode(&s["stderr"])).collect();
    for (index, step) in steps.iter().enumerate() {
        go_outs.push(Outcome {
            code: step["code"]
                .as_i64()
                .map(|c| i32::try_from(c).expect("code")),
            stdout: go_stdouts[index].clone(),
            stderr: go_stderrs[index].clone(),
        });
    }
    let recorded = Side {
        outs: go_outs.clone(),
        stdouts: go_stdouts,
        stderrs: go_stderrs,
        db: payload["db"]
            .as_array()
            .expect("recorded db")
            .iter()
            .map(|row| row.as_str().unwrap_or_default().to_owned())
            .collect(),
        tree: payload["tree"]
            .as_array()
            .expect("recorded tree")
            .iter()
            .map(|row| row.as_str().unwrap_or_default().to_owned())
            .collect(),
        requests: (Vec::new(), Vec::new()),
    };
    assert_sides(scenario, &recorded, &rust);
    let requests = |key: &str| -> Vec<RecordedRequest> {
        payload["requests"][key]
            .as_array()
            .map(|items| items.iter().map(request_from).collect())
            .unwrap_or_default()
    };
    Compared {
        go: go_outs,
        rust: rust.outs,
        go_requests: (requests("embed"), requests("llm")),
        rust_requests: rust.requests,
        db: recorded.db,
    }
}
