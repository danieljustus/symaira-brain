//! Differential harness for the native `symbrain memory` verbs (#758).
//!
//! The Go oracle is built from this worktree (or taken from
//! `SYMBRAIN_MEMORY_ORACLE`), both implementations run the identical argv in
//! identical, isolated HOME/XDG roots against identical database fixtures that
//! the Go oracle itself produced, and every observable is compared: exit code,
//! stdout and stderr bytes, the whole database state and a manifest of the
//! created files. The Rust process never has a Go fallback available.

#![allow(dead_code)]

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
    let out = uuid_re()
        .replace_all(&bytes, |caps: &regex::bytes::Captures<'_>| {
            let id = String::from_utf8_lossy(&caps[0]).into_owned();
            ids.label(&id).into_bytes()
        })
        .into_owned();
    out
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
            let meta = std::fs::symlink_metadata(&path).expect("metadata");
            let kind = if meta.is_dir() { "dir" } else { "file" };
            // WAL side files appear and vanish with connection state.
            if rel.ends_with("-wal") || rel.ends_with("-shm") || rel.ends_with("-journal") {
                continue;
            }
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
    /// Direct mutation of the fixture database (fault injection).
    pub mutate: Option<Box<dyn Fn(&Connection) + 'a>>,
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
            mutate: None,
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
}

fn expand(args: &[OsString], db: &Path) -> Vec<OsString> {
    args.iter()
        .map(|arg| {
            if arg == OsStr::new("{DB}") {
                db.as_os_str().to_owned()
            } else {
                arg.clone()
            }
        })
        .collect()
}

/// Runs a scenario against both implementations and panics with a precise
/// report on the first difference. Returns the observations.
pub fn compare(scenario: &Scenario<'_>) -> Compared {
    // Fixture: produced by the Go oracle itself.
    let seed_db: Option<PathBuf> = if scenario.no_db {
        None
    } else {
        let seed = Sandbox::new();
        let embed_server = scenario
            .embed
            .as_ref()
            .map(|h| FakeServer::start(Arc::clone(h)));
        let config = scenario
            .config
            .replace(
                "{EMBED}",
                &embed_server.as_ref().map_or_else(
                    || "owned-invalid-endpoint".to_owned(),
                    |s| format!("{}/api/embeddings", s.url),
                ),
            )
            .replace("{LLM}", "http://127.0.0.1:1");
        seed.write_config(&config);
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
        if let Some(mutate) = &scenario.mutate {
            let conn = Connection::open(seed.db()).expect("mutate connection");
            mutate(&conn);
        }
        // Checkpoint into the main file so the copy is complete.
        {
            let conn = Connection::open(seed.db()).expect("checkpoint connection");
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        }
        let kept = std::env::temp_dir().join(format!(
            "memnat-seed-{}-{}.db",
            std::process::id(),
            scenario.name
        ));
        std::fs::copy(seed.db(), &kept).expect("seed copy");
        Some(kept)
    };

    let mut compared = Compared {
        go: Vec::new(),
        rust: Vec::new(),
        go_requests: (Vec::new(), Vec::new()),
        rust_requests: (Vec::new(), Vec::new()),
    };
    let mut snapshots: Vec<(Vec<String>, Vec<String>, Vec<Vec<u8>>, Vec<Vec<u8>>)> = Vec::new();
    let known = seed_db.as_deref().map(known_ids).unwrap_or_default();
    for who in [Who::Go, Who::Rust] {
        let sb = Sandbox::new();
        let embed_server = scenario
            .embed
            .as_ref()
            .map(|h| FakeServer::start(Arc::clone(h)));
        let llm_server = scenario
            .llm
            .as_ref()
            .map(|h| FakeServer::start(Arc::clone(h)));
        let config = scenario
            .config
            .replace(
                "{EMBED}",
                &embed_server.as_ref().map_or_else(
                    || "owned-invalid-endpoint".to_owned(),
                    |s| format!("{}/api/embeddings", s.url),
                ),
            )
            .replace(
                "{LLM}",
                &llm_server
                    .as_ref()
                    .map_or_else(|| "http://127.0.0.1:1".to_owned(), |s| s.url.clone()),
            );
        sb.write_config(&config);
        if let Some(seed) = &seed_db {
            std::fs::copy(seed, sb.db()).expect("fixture copy");
        }
        let mut ids = IdMap::with_known(known.clone());
        let mut outs = Vec::new();
        for step in &scenario.steps {
            let out = run(
                who,
                &sb,
                scenario.rust_bin,
                &expand(step, &sb.db()),
                &scenario.env,
            );
            outs.push(out);
        }
        let normalized: Vec<(Vec<u8>, Vec<u8>)> = outs
            .iter()
            .map(|out| {
                (
                    normalize_output(&out.stdout, &sb.root, &mut ids),
                    normalize_output(&out.stderr, &sb.root, &mut ids),
                )
            })
            .collect();
        let db_state = snapshot(&sb.db(), &mut ids);
        let tree = manifest(&sb.root);
        let (stdouts, stderrs): (Vec<_>, Vec<_>) = normalized.into_iter().unzip();
        snapshots.push((db_state, tree, stdouts, stderrs));
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
        match who {
            Who::Go => {
                compared.go = outs;
                compared.go_requests = requests;
            }
            Who::Rust => {
                compared.rust = outs;
                compared.rust_requests = requests;
            }
        }
    }

    let (go_db, go_tree, go_out, go_err) = &snapshots[0];
    let (rs_db, rs_tree, rs_out, rs_err) = &snapshots[1];
    for (index, (go, rust)) in compared.go.iter().zip(&compared.rust).enumerate() {
        assert_eq!(
            go.code,
            rust.code,
            "{} step {index}: exit status\n go stderr: {}\n rust stderr: {}",
            scenario.name,
            String::from_utf8_lossy(&go.stderr),
            String::from_utf8_lossy(&rust.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&go_out[index]),
            String::from_utf8_lossy(&rs_out[index]),
            "{} step {index}: stdout",
            scenario.name
        );
        assert_eq!(
            go_out[index], rs_out[index],
            "{} step {index}: stdout bytes",
            scenario.name
        );
        if scenario.stderr_as_set {
            let mut a: Vec<&[u8]> = go_err[index].split(|b| *b == b'\n').collect();
            let mut b: Vec<&[u8]> = rs_err[index].split(|b| *b == b'\n').collect();
            a.sort_unstable();
            b.sort_unstable();
            assert_eq!(a, b, "{} step {index}: stderr lines", scenario.name);
        } else {
            assert_eq!(
                String::from_utf8_lossy(&go_err[index]),
                String::from_utf8_lossy(&rs_err[index]),
                "{} step {index}: stderr",
                scenario.name
            );
            assert_eq!(
                go_err[index], rs_err[index],
                "{} step {index}: stderr bytes",
                scenario.name
            );
        }
    }
    assert_eq!(go_db, rs_db, "{}: database state", scenario.name);
    assert_eq!(go_tree, rs_tree, "{}: created files", scenario.name);
    if let Some(seed) = seed_db {
        let _ = std::fs::remove_file(seed);
    }
    compared
}
