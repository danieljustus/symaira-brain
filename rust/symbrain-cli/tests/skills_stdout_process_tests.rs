//! Actual process stdout failures of `symbrain skills` reports (#830).
//!
//! `std::io::Stdout` treats EBADF as success; the skills reports must instead
//! fail like Go's `os.Stdout`, keep the sink bytes intact and still die by
//! SIGPIPE when the reader is gone.

#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::process::{Command, Stdio};

use tempfile::TempDir;

const REPORTS: [&[&str]; 6] = [
    &["list"],
    &["status"],
    &["targets"],
    &["log"],
    &["sync", "--dry-run"],
    &["doctor"],
];

fn command(root: &TempDir, report: &[&str], format: &str) -> Command {
    let home = root.path().join("home");
    let project = root.path().join("project");
    for name in ["home", "config", "data", "cache", "project"] {
        std::fs::create_dir_all(root.path().join(name)).unwrap();
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .env_clear()
        .envs(coverage::profile_environment())
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("XDG_DATA_HOME", root.path().join("data"))
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C.UTF-8")
        .current_dir(project)
        .arg("skills")
        .args(report)
        .args(["--output", format])
        .stdin(Stdio::null())
        .stderr(Stdio::piped());
    command
}

#[test]
fn readonly_stdout_descriptor_fails_with_bad_file_descriptor() {
    for format in ["table", "json"] {
        for report in REPORTS {
            let root = TempDir::new().unwrap();
            let sentinel = root.path().join("sentinel");
            std::fs::write(&sentinel, b"sentinel").unwrap();
            let readonly = std::fs::File::open(&sentinel).unwrap();
            let output = command(&root, report, format)
                .stdout(Stdio::from(readonly))
                .output()
                .unwrap();
            let label = format!("skills {report:?} --output {format}");
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(1), "{label}: {stderr}");
            assert!(
                stderr.contains("write /dev/stdout: bad file descriptor"),
                "{label}: {stderr}"
            );
            assert_eq!(std::fs::read(&sentinel).unwrap(), b"sentinel", "{label}");
        }
    }
}

#[test]
fn closed_reader_terminates_with_sigpipe() {
    use std::os::unix::process::ExitStatusExt;

    for format in ["table", "json"] {
        for report in REPORTS {
            let root = TempDir::new().unwrap();
            let mut child = command(&root, report, format)
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            drop(child.stdout.take());
            let output = child.wait_with_output().unwrap();
            let label = format!("skills {report:?} --output {format}");
            assert_eq!(
                output.status.signal(),
                Some(signal_hook::consts::SIGPIPE),
                "{label}: {:?}",
                output.stderr
            );
        }
    }
}

#[test]
fn writable_stdout_still_receives_the_report() {
    for report in REPORTS {
        let root = TempDir::new().unwrap();
        let output = command(&root, report, "json")
            .stdout(Stdio::piped())
            .output()
            .unwrap();
        assert!(output.status.success(), "{report:?}: {:?}", output.stderr);
        assert!(output.stdout.ends_with(b"\n"), "{report:?}");
        assert!(!output.stdout.is_empty(), "{report:?}");
    }
}
