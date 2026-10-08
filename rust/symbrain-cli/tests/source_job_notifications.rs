//! Owned Windows diagnosis of polling, cached exit status and descendant cleanup.
#![cfg(windows)]

use process_wrap::std::{ChildWrapper, CommandWrap, JobObject};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[path = "../src/setup_source_windows_job.rs"]
mod source_job;

fn event(root: &Path, phase: &str, pid: u32) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("phases.jsonl"))
        .unwrap();
    writeln!(file, "{{\"phase\":\"{phase}\",\"pid\":{pid}}}").unwrap();
    file.sync_all().unwrap();
    println!("owned phase={phase} pid={pid}");
}

fn event_result<T>(root: &Path, phase: &str, pid: u32, result: &std::io::Result<T>) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("phases.jsonl"))
        .unwrap();
    serde_json::to_writer(
        &mut file,
        &serde_json::json!({"phase": phase, "pid": pid, "ok": result.is_ok(),
                           "error": result.as_ref().err().map(ToString::to_string)}),
    )
    .unwrap();
    writeln!(file).unwrap();
    file.sync_all().unwrap();
    println!("owned phase={phase} pid={pid} ok={}", result.is_ok());
}

fn command(root: &Path, mode: &str) -> Command {
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args([
            "owned_job_diagnostic_helper",
            "--exact",
            "--ignored",
            "--nocapture",
        ])
        .env("SOURCE_JOB_DIAGNOSTIC_ROOT", root)
        .env("SOURCE_JOB_DIAGNOSTIC_MODE", mode)
        .stdin(Stdio::null())
        .stdout(fs::File::create(root.join(format!("{mode}.stdout"))).unwrap())
        .stderr(fs::File::create(root.join(format!("{mode}.stderr"))).unwrap());
    child
}

struct HistoricalOwned(Box<dyn ChildWrapper>, std::path::PathBuf);
impl Drop for HistoricalOwned {
    fn drop(&mut self) {
        // Exact existing setup_source_process::Owned Windows cleanup calls.
        event(&self.1, "historical-kill-enter", self.0.id());
        let killed = self.0.kill();
        event_result(&self.1, "historical-kill-returned", self.0.id(), &killed);
        event(&self.1, "historical-wait-enter", self.0.id());
        let waited = self.0.wait();
        event_result(&self.1, "historical-wait-returned", self.0.id(), &waited);
    }
}

// The native JobObject experiment requires a successful parent with a still
// live child. The wrapper owns termination; the external watchdog retains and
// forcibly cleans the recorded owned PID if that contract fails.
#[allow(clippy::zombie_processes)]
fn owned_live_descendant(root: &Path) {
    let child = command(root, "held").spawn().unwrap();
    fs::write(root.join("descendant.pid"), child.id().to_string()).unwrap();
    event(root, "descendant-created", child.id());
}

#[test]
#[ignore = "owned Windows watchdog required; deliberately blocks historical cleanup"]
fn owned_job_diagnostic_helper() {
    let root = std::path::PathBuf::from(std::env::var_os("SOURCE_JOB_DIAGNOSTIC_ROOT").unwrap());
    let mode = std::env::var("SOURCE_JOB_DIAGNOSTIC_MODE").unwrap();
    if mode == "quick" {
        println!("owned quick stdout");
        eprintln!("owned quick stderr");
        return;
    }
    if mode == "held" {
        event(&root, "held-live", std::process::id());
        thread::sleep(Duration::from_secs(60));
        return;
    }
    if mode == "with-descendant" {
        owned_live_descendant(&root);
        return;
    }
    assert!(matches!(
        mode.as_str(),
        "historical" | "inner-wait" | "descendant" | "source-cleanup" | "source-descendant"
    ));
    let source_mode = mode.starts_with("source-");
    let child_mode = if matches!(mode.as_str(), "descendant" | "source-descendant") {
        "with-descendant"
    } else {
        "quick"
    };
    let mut child = CommandWrap::from(command(&root, child_mode))
        .wrap(JobObject)
        .spawn()
        .unwrap();
    event(&root, "wrapper-created", child.id());
    let started = Instant::now();
    loop {
        let status = if source_mode {
            source_job::poll_parent(child.as_mut())
        } else {
            child.try_wait()
        };
        if let Some(status) = status.unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "owned quick parent did not exit"
        );
        thread::sleep(Duration::from_millis(10));
    }
    event(&root, "parent-exited", child.id());
    // Original comparison modes consume wrapper notifications. Source modes
    // repeat the exact parent-only production poll and leave job events intact.
    for _ in 0..16 {
        let status = if source_mode {
            source_job::poll_parent(child.as_mut())
        } else {
            child.try_wait()
        };
        assert!(status.unwrap().unwrap().success());
        thread::sleep(Duration::from_millis(10));
    }
    event(
        &root,
        if source_mode {
            "post-exit-parent-polled"
        } else {
            "post-exit-notifications-polled"
        },
        child.id(),
    );
    if mode == "historical" {
        event(&root, "historical-drop-enter", child.id());
        drop(HistoricalOwned(child, root.clone()));
    } else if source_mode {
        // Exercise the exact production helper, retaining full JobObject wait.
        event(&root, "source-cleanup-enter", child.id());
        let waited = source_job::terminate_and_reap(child.as_mut());
        event_result(&root, "source-cleanup-returned", child.id(), &waited);
        assert!(waited.unwrap().success());
        drop(child);
    } else {
        // A diagnostic comparison, not a production correction. Kill the
        // complete owned job; await only the cached top-level child status.
        event(&root, "job-termination-enter", child.id());
        let terminated = child.start_kill();
        event_result(&root, "job-termination-returned", child.id(), &terminated);
        terminated.unwrap();
        event(&root, "job-termination-requested", child.id());
        event(&root, "inner-wait-enter", child.id());
        let waited = child.inner_mut().wait();
        event_result(&root, "inner-wait-result", child.id(), &waited);
        assert!(waited.unwrap().success());
        event(&root, "inner-wait-returned", child.id());
        drop(child);
    }
    event(&root, "wrapper-drop-returned", std::process::id());
}
