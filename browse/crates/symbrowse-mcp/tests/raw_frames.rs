#![deny(unsafe_code)]

use std::{
    fs,
    io::Cursor,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde_json::json;
use symbrowse_daemon::{
    DaemonError, DaemonHandler, Frame, HandlerResult, OperationContext, Server, ServerOptions,
};
use symbrowse_mcp::{ServeOptions, serve_stdio};

struct FixtureDaemon {
    server: Arc<Server>,
    worker: Option<JoinHandle<Result<(), symbrowse_daemon::ServerError>>>,
    endpoint: PathBuf,
    session: String,
    root: PathBuf,
}

#[allow(clippy::result_large_err)]
fn fixture_handler(frame: Frame, _context: OperationContext) -> HandlerResult {
    if frame.cmd == "fetch.url" {
        return Err(DaemonError {
            code: "fixture_denied".into(),
            message: "fixture daemon denied the request".into(),
            hint: "use an allowed fixture target".into(),
            details: Some(json!({"fixture": true})),
            retryable: Some(false),
            requires_user_confirmation: Some(false),
            resume_hint: "choose an allowed target".into(),
        });
    }
    Ok((Some(json!({"fixture": true})), Vec::new()))
}

impl FixtureDaemon {
    fn start() -> Self {
        static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
        #[cfg(unix)]
        let root_parent = PathBuf::from("/tmp");
        #[cfg(windows)]
        let root_parent = std::env::temp_dir();
        let mut suffix = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let (_endpoint_suffix, root) = loop {
            let endpoint_suffix = format!("mcp-fixture-{}-{suffix}", std::process::id());
            let root = root_parent.join(format!("sbmcp-{endpoint_suffix}"));
            match fs::create_dir(&root) {
                Ok(()) => break (endpoint_suffix, root),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    suffix = suffix.checked_add(1).expect("fixture root suffix overflow");
                }
                Err(error) => panic!("create isolated fixture root: {error}"),
            }
        };
        #[cfg(unix)]
        let endpoint = root.join("daemon.sock");
        #[cfg(windows)]
        let endpoint = PathBuf::from(format!(r"\\.\pipe\symbrowse-{_endpoint_suffix}"));
        let session = "default".to_owned();

        let handler: DaemonHandler = Arc::new(fixture_handler);
        let server = Arc::new(
            Server::new(ServerOptions {
                socket_path: endpoint.clone(),
                session: session.clone(),
                handler: Some(handler),
                registry: Some(Arc::new(symbrowse_daemon::SessionRegistry::new(
                    symbrowse_daemon::SessionRegistryOptions {
                        user_data_root: root.join("sessions"),
                        pid: std::process::id(),
                        scope: String::new(),
                        origin_path: String::new(),
                    },
                ))),
                ..ServerOptions::default()
            })
            .expect("fixture daemon"),
        );
        let running = server.clone();
        let worker = thread::spawn(move || running.listen_and_serve());
        let daemon = Self {
            server,
            worker: Some(worker),
            endpoint,
            session,
            root,
        };
        daemon.wait_until_ready();
        daemon
    }

    fn wait_until_ready(&self) {
        let client = symbrowse_daemon::Client::new(symbrowse_daemon::ClientOptions {
            socket_path: self.endpoint.clone(),
            session: self.session.clone(),
            read_timeout: Duration::from_millis(200),
            startup_timeout: Duration::from_millis(200),
            autostart: false,
            start: None,
            expected_engine: None,
            expected_policy: None,
        });
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            match client.request_without_autostart(Frame {
                cmd: "daemon.status".into(),
                session: self.session.clone(),
                ..Frame::default()
            }) {
                Ok(response) if response.success => return,
                _ => thread::sleep(Duration::from_millis(10)),
            }
        }
        panic!(
            "fixture daemon did not become ready at {}",
            self.endpoint.display()
        );
    }
}

impl Drop for FixtureDaemon {
    fn drop(&mut self) {
        self.server.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn run_fixture(name: &str, args: &[&str], daemon: &FixtureDaemon) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let input_path = format!("../../testdata/port/mcp/{name}.in");
    let output_path = format!("../../testdata/port/mcp/{name}.out");
    let input = std::fs::read(root.join(input_path)).expect("read MCP input fixture");
    let expected = std::fs::read(root.join(output_path)).expect("read MCP output fixture");
    let mut profiles = "core";
    for window in args.windows(2) {
        if window[0] == "--tools" {
            profiles = window[1];
        }
    }
    let options = ServeOptions {
        version: "v0.8.0".to_owned(),
        session: daemon.session.clone(),
        profiles: profiles.to_owned(),
        executable: String::new(),
        allow_private: false,
        engine: None,
        daemon_log_path: None,
        endpoint: Some(daemon.endpoint.to_string_lossy().into_owned()),
    };
    let mut actual = Vec::new();
    // Every fixture, including policy failures, must traverse the production
    // daemon proxy. A test-only proxy can make Rust agree with an expected
    // file without exercising transport, policy, or platform endpoint code.
    serve_stdio(Cursor::new(input), &mut actual, options).expect("serve MCP fixture");
    if actual != expected {
        let offset = actual
            .iter()
            .zip(&expected)
            .position(|(actual, expected)| actual != expected)
            .unwrap_or(actual.len().min(expected.len()));
        let actual_end = offset.saturating_add(32).min(actual.len());
        let expected_end = offset.saturating_add(32).min(expected.len());
        panic!(
            "fixture={name} differs at byte {offset} (actual {}, expected {}); actual excerpt {:?}, expected excerpt {:?}",
            actual.len(),
            expected.len(),
            String::from_utf8_lossy(&actual[offset.min(actual.len())..actual_end]),
            String::from_utf8_lossy(&expected[offset.min(expected.len())..expected_end]),
        );
    }
}

#[test]
fn initialize_is_byte_exact_and_clean() {
    let daemon = FixtureDaemon::start();
    run_fixture("initialize", &["mcp"], &daemon);
    run_fixture("framed_initialize", &["mcp"], &daemon);
    run_fixture("initialized", &["mcp"], &daemon);
}

#[test]
fn tools_list_profiles_are_byte_exact() {
    let daemon = FixtureDaemon::start();
    run_fixture("tools_core", &["mcp"], &daemon);
    run_fixture("tools_nav", &["mcp", "--tools", "nav"], &daemon);
    run_fixture("tools_all", &["mcp", "--tools", "all"], &daemon);
}

#[test]
fn malformed_unknown_and_typed_argument_errors_match() {
    let daemon = FixtureDaemon::start();
    run_fixture("malformed", &["mcp"], &daemon);
    run_fixture("unknown_method", &["mcp"], &daemon);
    run_fixture("unknown_tool", &["mcp"], &daemon);
    run_fixture("missing_argument", &["mcp"], &daemon);
    run_fixture("tool_error", &["mcp"], &daemon);
}

#[test]
fn notifications_and_eof_are_silent() {
    let daemon = FixtureDaemon::start();
    run_fixture("notifications", &["mcp"], &daemon);
    run_fixture("eof", &["mcp"], &daemon);
}
