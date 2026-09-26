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
    time::{Duration, Instant},
};

use serde_json::json;
use symbrowse_daemon::{Client, ClientOptions, Frame, Server, ServerOptions, SessionSpec};

fn enabled() -> bool {
    std::env::var_os("SYMBROWSE_E2E").as_deref() == Some(std::ffi::OsStr::new("1"))
}

struct LoginFixture {
    origin: String,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl LoginFixture {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind auth login fixture");
        let address = listener.local_addr().expect("auth fixture address");
        listener
            .set_nonblocking(true)
            .expect("set auth fixture nonblocking");
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !stop_for_thread.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => respond(&mut stream),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            origin: format!("http://{address}"),
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for LoginFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn respond(stream: &mut TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set auth fixture read timeout");
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];
    while request.len() < 8192 && !request.windows(4).any(|part| part == b"\r\n\r\n") {
        let Ok(read) = stream.read(&mut chunk) else {
            return;
        };
        if read == 0 {
            return;
        }
        request.extend_from_slice(&chunk[..read]);
    }
    if !request.windows(4).any(|part| part == b"\r\n\r\n") {
        return;
    }
    let body = "<!doctype html><form><input id='user' type='email'><input id='pass' type='password'></form>";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn build_fake_symvault(directory: &std::path::Path) -> PathBuf {
    let source = directory.join("fake_symvault.go");
    let executable = directory.join(if cfg!(windows) {
        "symvault.exe"
    } else {
        "symvault"
    });
    fs::write(
        &source,
        r#"package main
import (
  "fmt"
  "os"
)
func main() {
  if len(os.Args) != 3 || os.Args[1] != "get" || os.Args[2] != "fixture-entry" { os.Exit(2) }
  fmt.Print(`{"username":"fixture-user","password":"fixture-pass"}`)
}
"#,
    )
    .expect("write fake Symvault source");
    let output = Command::new("go")
        .args(["build", "-trimpath", "-o"])
        .arg(&executable)
        .arg(&source)
        .env("GO111MODULE", "off")
        .output()
        .expect("build fake Symvault executable with the installed Go toolchain");
    assert!(output.status.success(), "fake Symvault build failed");
    executable
}

fn install_fake_symvault_on_path(directory: &std::path::Path) {
    let old_path = std::env::var_os("PATH").unwrap_or_default();
    let paths = std::iter::once(directory.to_path_buf()).chain(std::env::split_paths(&old_path));
    let path = std::env::join_paths(paths).expect("join test PATH");
    // This test is the only case in its own integration-test executable and
    // sets PATH before creating the Tokio runtime or starting worker threads.
    unsafe { std::env::set_var("PATH", path) };
}

#[test]
fn production_daemon_auth_login_uses_fake_vault_and_fixture_chrome() {
    if !enabled() {
        return;
    }
    let root = tempfile::tempdir().expect("isolated auth E2E root");
    let chrome_unavailable = std::env::var_os("SYMBROWSE_E2E_EXPECT_CHROME_UNAVAILABLE")
        .is_some_and(|value| value == "1");
    if !chrome_unavailable {
        let vault_dir = root.path().join("fake-vault-bin");
        fs::create_dir(&vault_dir).expect("create fake vault bin directory");
        let vault_executable = build_fake_symvault(&vault_dir);
        assert!(vault_executable.is_file());
        install_fake_symvault_on_path(&vault_dir);
    }

    let fixture = LoginFixture::start();
    let session = format!("native-auth-{}", std::process::id());
    let mut spec = SessionSpec::for_session(&session);
    spec.mode = "browser".into();
    spec.engine = "chrome".into();
    spec.state_dir = root.path().join("state");
    spec.cache_dir = root.path().join("cache");
    spec.daemon_log = root.path().join("daemon.log");
    spec.allowed_domains = vec!["127.0.0.1".into()];
    spec.allow_private = true;
    spec.operation_timeout = Duration::from_secs(45);
    spec.idle_timeout = Some(Duration::from_secs(30));
    let profile = spec.user_data_dir();
    let mut private_socket_dir = None;
    if cfg!(windows) {
        spec.socket_path = symbrowse_daemon::default_socket_path(&session);
    } else if cfg!(target_os = "macos") {
        let directory =
            PathBuf::from("/private/tmp").join(format!("sbauth-{}", std::process::id()));
        fs::create_dir(&directory).expect("create private auth socket directory");
        spec.socket_path = directory.join("daemon.sock");
        private_socket_dir = Some(directory);
    } else {
        spec.socket_path = root.path().join("daemon.sock");
    }

    let server = Arc::new(
        Server::new(ServerOptions {
            session_spec: Some(spec.clone()),
            operation_timeout: Duration::from_secs(45),
            idle_timeout: Some(Duration::from_secs(30)),
            ..ServerOptions::default()
        })
        .expect("create isolated auth daemon"),
    );
    let server_for_thread = Arc::clone(&server);
    let thread = thread::spawn(move || {
        server_for_thread
            .listen_and_serve()
            .expect("serve isolated auth daemon")
    });
    let client = Client::new(ClientOptions {
        socket_path: spec.socket_path.clone(),
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
            "auth daemon did not become ready"
        );
        thread::sleep(Duration::from_millis(20));
    }

    let unsafe_target = client
        .request(Frame {
            cmd: "auth.login".into(),
            args: Some(json!({
                "entry":"fixture-entry",
                "url":"file:///private/auth-fixture",
            })),
            session: session.clone(),
            ..Frame::default()
        })
        .expect("submit denied auth target");
    let unsafe_target = serde_json::to_value(unsafe_target).expect("encode denied auth response");
    assert_eq!(unsafe_target["success"], false);
    assert_eq!(unsafe_target["error"]["code"], "operation_failed");
    assert!(
        unsafe_target["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("http/https URL required"))
    );

    if chrome_unavailable {
        server.stop();
        thread.join().expect("join unavailable auth daemon");
        drop(server);
        drop(fixture);
        if let Some(directory) = private_socket_dir {
            fs::remove_dir_all(directory).expect("remove private auth socket directory");
        }
        return;
    }

    let _chrome = std::env::var_os("SYMBROWSE_CHROME_EXECUTABLE")
        .expect("native auth E2E requires isolated Chrome for Testing");
    let chrome_version = std::env::var("SYMBROWSE_CHROME_VERSION")
        .expect("native auth E2E records the tested Chrome version");
    assert!(!chrome_version.is_empty());
    eprintln!(
        "native_auth_e2e=chrome version={chrome_version} arch={}",
        std::env::consts::ARCH
    );

    let response = client
        .request(Frame {
            cmd: "auth.login".into(),
            args: Some(json!({
                "entry":"fixture-entry",
                "url":format!("{}/login", fixture.origin),
            })),
            session: session.clone(),
            ..Frame::default()
        })
        .expect("send auth.login frame");
    let response = serde_json::to_value(response).expect("encode auth response");
    assert_eq!(response["success"], true, "auth.login response: {response}");
    assert_eq!(response["data"]["status"], "logged_in");
    assert_eq!(response["data"]["url"], fixture.origin);
    assert_eq!(response["data"]["username_set"], true);
    assert_eq!(response["data"]["password_set"], true);
    let serialized = response.to_string();
    assert!(!serialized.contains("fixture-user"));
    assert!(!serialized.contains("fixture-pass"));

    let fields = client
        .request(Frame {
            cmd: "eval".into(),
            args: Some(json!({
                "expression":"document.querySelector('#user').value.length > 0 && document.querySelector('#pass').value.length > 0"
            })),
            session: session.clone(),
            ..Frame::default()
        })
        .expect("verify fixture form fields");
    let fields = serde_json::to_value(fields).expect("encode field verification");
    assert_eq!(fields["success"], true, "field verification failed");
    assert_eq!(
        fields["data"]["value"], true,
        "fixture fields were not filled"
    );

    let journal = client
        .request(Frame {
            cmd: "journal.tail".into(),
            args: Some(json!({"lines":100})),
            session: session.clone(),
            ..Frame::default()
        })
        .expect("read auth action journal");
    let journal = serde_json::to_value(journal).expect("encode journal response");
    let journal = journal.to_string();
    assert!(!journal.contains("fixture-user"));
    assert!(!journal.contains("fixture-pass"));

    server.stop();
    thread.join().expect("join auth daemon");
    assert!(
        fs::read_to_string(&spec.daemon_log)
            .unwrap_or_default()
            .find("fixture-pass")
            .is_none(),
        "fake password was written to the daemon log"
    );
    drop(server);
    drop(fixture);
    assert!(
        profile.is_dir(),
        "browser profile was not isolated under test root"
    );
    if let Some(directory) = private_socket_dir {
        fs::remove_dir_all(directory).expect("remove private auth socket directory");
    }
}
