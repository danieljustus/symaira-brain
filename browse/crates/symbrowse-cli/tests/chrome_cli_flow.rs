//! Real-Chrome `open` + `read` through the `symbrowse` binary and its
//! autostarted daemon, the path a user and PERF-003 take (#780).
//!
//! The daemon-level Chrome tests drive the server in-process and did not catch
//! that a CLI-spawned daemon held every `open` until its operation timeout, nor
//! that `read` returned plain text where the CLI renders Go's `{html, title,
//! url}`. Opt-in: `SYMBROWSE_E2E=1` and `SYMBROWSE_CHROME_EXECUTABLE`.

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

use serde_json::Value;

const TITLE: &str = "CLI flow fixture";
const TOKEN: &str = "cli-flow-token-5150";
/// A well-formed throwaway state key, so no host key provider is consulted.
const STATE_KEY: &str = "c11f10c11f10c11f10c11f10c11f10c11f10c11f10c11f10c11f10c11f10c11f";

fn serve_fixture() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture");
    let address = listener.local_addr().expect("fixture address");
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request);
            let body = format!(
                "<!doctype html><html><head><title>{TITLE}</title></head>\
                 <body><main><h1>{TITLE}</h1><p>{TOKEN}</p></main></body></html>"
            );
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    format!("http://{address}/")
}

fn run(root: &Path, chrome: &str, args: &[&str]) -> (Output, Duration) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrowse"));
    command
        .args(args)
        .args(["--json", "--session", "cliflow"])
        .current_dir(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", root.join("h"))
        .env("XDG_CONFIG_HOME", root.join("c"))
        .env("XDG_CACHE_HOME", root.join("k"))
        .env("XDG_DATA_HOME", root.join("d"))
        .env("XDG_STATE_HOME", root.join("s"))
        .env("XDG_RUNTIME_DIR", root.join("r"))
        .env("TMPDIR", root.join("t"))
        .env("SYMBROWSE_MODE", "browser")
        .env("SYMBROWSE_ENGINE", "chrome")
        .env("SYMBROWSE_CHROME_EXECUTABLE", chrome)
        .env("SYMBROWSE_HEADLESS", "1")
        .env("SYMBROWSE_ALLOW_PRIVATE", "true")
        .env("SYMBROWSE_CHECK_UPDATES", "0")
        .env("SYMBROWSE_SYMGUARD", "off")
        .env("SYMBROWSE_OOB_NOTIFY", "off")
        .env("SYMBROWSE_ENCRYPTION_KEY", STATE_KEY)
        .stdin(Stdio::null());
    let started = Instant::now();
    let output = command.output().expect("run symbrowse");
    (output, started.elapsed())
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not JSON ({error}): {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

struct Root(PathBuf);

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn chrome_open_and_read_through_the_cli() {
    if std::env::var_os("SYMBROWSE_E2E").is_none() {
        return;
    }
    let chrome = std::env::var("SYMBROWSE_CHROME_EXECUTABLE")
        .expect("SYMBROWSE_E2E requires SYMBROWSE_CHROME_EXECUTABLE");
    // Short path: macOS AF_UNIX socket paths are limited to 104 bytes.
    let root = Root(PathBuf::from(format!("/tmp/sbcf-{}", std::process::id())));
    for dir in ["h", "c", "k", "d", "s", "r", "t"] {
        fs::create_dir_all(root.0.join(dir)).expect("create isolated root");
    }
    let url = serve_fixture();

    let (opened, elapsed) = run(&root.0, &chrome, &["open", &url]);
    let opened_json = json(&opened);
    assert_eq!(opened_json["success"], true, "open: {opened_json}");
    assert_eq!(
        opened_json["data"]["url"],
        url.as_str(),
        "open: {opened_json}"
    );
    assert_eq!(
        opened_json["data"]["http_status"], 200,
        "open: {opened_json}"
    );
    // Launch included; the broken path only answered at the 25 s deadline.
    assert!(elapsed < Duration::from_secs(20), "open took {elapsed:?}");

    let (read, _) = run(&root.0, &chrome, &["read"]);
    let text = String::from_utf8_lossy(&read.stdout);
    assert!(read.status.success(), "read: {text}");
    assert!(text.contains(TITLE), "read lost the title: {text}");
    assert!(text.contains(TOKEN), "read lost the content: {text}");
    assert!(text.contains(url.as_str()), "read lost the URL: {text}");

    let (stopped, _) = run(&root.0, &chrome, &["daemon", "stop"]);
    assert!(stopped.status.success(), "daemon stop: {stopped:?}");
}
