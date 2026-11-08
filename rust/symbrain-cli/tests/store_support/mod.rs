//! Shared helpers for the process-level store-creation tests: a bounded child
//! wait that fails instead of hanging (#917) and explicit scenario reporting
//! so privilege-dependent skips are never silent passes (#936).

#![allow(dead_code)] // each test binary uses a different subset

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Upper bound for one CLI child. Generous for loaded CI, finite for a hang.
pub const CHILD_TIMEOUT: Duration = Duration::from_secs(120);

/// Disposable canonical root: Go reports the resolved path of trusted links.
pub fn temp() -> (tempfile::TempDir, PathBuf) {
    let guard = tempfile::tempdir().expect("root");
    let root = guard.path().canonicalize().expect("canonical root");
    (guard, root)
}

/// Confines a child to `root`: cleared environment, private XDG roots.
pub fn isolate<'a>(command: &'a mut Command, root: &Path) -> &'a mut Command {
    command
        .env_clear()
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("PATH", "")
        .env("SYMBRAIN_GO_BINARY", root.join("absent-go"))
}

fn drain(mut stream: impl Read + Send + 'static) -> mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stream.read_to_end(&mut bytes);
        let _ = sender.send(bytes);
    });
    receiver
}

/// Runs `command` to completion within `limit`; a child that outlives it is
/// killed and the test fails with a timeout, so a blocked open can never hang
/// `cargo test`.
pub fn output_within(command: &mut Command, limit: Duration) -> Output {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn CLI child");
    let stdout = drain(child.stdout.take().expect("stdout pipe"));
    let stderr = drain(child.stderr.take().expect("stderr pipe"));
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll CLI child") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("CLI child timed out after {limit:?}: {command:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let collect = |receiver: mpsc::Receiver<Vec<u8>>| {
        receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("child output pipe closed")
    };
    Output {
        status,
        stdout: collect(stdout),
        stderr: collect(stderr),
    }
}

/// One line of the scenario report.
pub fn report_line(outcome: &str, scenario: &str, reason: &str) -> String {
    if reason.is_empty() {
        format!("{outcome} {scenario}\n")
    } else {
        format!("{outcome} {scenario}: {reason}\n")
    }
}

fn append_report(path: &Path, line: &str) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

fn report(outcome: &str, scenario: &str, reason: &str) {
    let line = report_line(outcome, scenario, reason);
    if let Some(path) = std::env::var_os("SYMBRAIN_TEST_SCENARIO_REPORT") {
        append_report(Path::new(&path), &line);
    }
    if outcome != "executed" {
        // A direct handle write is not swallowed by libtest's output capture,
        // unlike `eprintln!`, so the skip shows up in a default `cargo test`.
        let _ = std::io::stderr().write_all(line.as_bytes());
    }
}

/// Records that `scenario` ran, for the optional scenario report file.
pub fn executed(scenario: &str) {
    report("executed", scenario, "");
}

/// Reports a scenario the current privileges cannot exercise. It is printed
/// even on success and listed as skipped (never as executed) in the report;
/// `SYMBRAIN_DENY_PRIVILEGE_SKIPS=1` turns it into a failure for runners that
/// are required to prove the scenario.
pub fn skip_privileged(scenario: &str, reason: &str) {
    report("skipped(privilege)", scenario, reason);
    assert!(
        std::env::var_os("SYMBRAIN_DENY_PRIVILEGE_SKIPS").is_none(),
        "privilege-dependent scenario {scenario} was required but cannot run: {reason}"
    );
}
