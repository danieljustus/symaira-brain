//! Owned Windows diagnosis of polling, cached exit status and descendant cleanup.
#![cfg(windows)]

use process_wrap::std::{ChildWrapper, CommandWrap, JobObject};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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

struct HistoricalOwned(Box<dyn ChildWrapper>);
impl Drop for HistoricalOwned {
    fn drop(&mut self) {
        // Exact existing setup_source_process::Owned Windows cleanup calls.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
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
        let child = command(&root, "held").spawn().unwrap();
        fs::write(root.join("descendant.pid"), child.id().to_string()).unwrap();
        event(&root, "descendant-created", child.id());
        return;
    }
    assert!(matches!(
        mode.as_str(),
        "historical" | "inner-wait" | "descendant"
    ));
    let child_mode = if mode == "descendant" {
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
        if let Some(status) = child.try_wait().unwrap() {
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
    // Consume any remaining notifications after the exit, while asserting the
    // documented cached-status contract on every further poll.
    for _ in 0..16 {
        assert!(child.try_wait().unwrap().unwrap().success());
        thread::sleep(Duration::from_millis(10));
    }
    event(&root, "post-exit-notifications-polled", child.id());
    if mode == "historical" {
        event(&root, "historical-drop-enter", child.id());
        drop(HistoricalOwned(child));
    } else {
        // A diagnostic comparison, not a production correction. Kill the
        // complete owned job; await only the cached top-level child status.
        child.start_kill().unwrap();
        event(&root, "job-termination-requested", child.id());
        assert!(child.inner_mut().wait().unwrap().success());
        event(&root, "inner-wait-returned", child.id());
        drop(child);
    }
    event(&root, "wrapper-drop-returned", std::process::id());
}
