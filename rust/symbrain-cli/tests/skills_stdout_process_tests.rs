//! Exercise the executable's real readonly stdout handle in both report formats.
#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::fs;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn all_skills_reports_surface_readonly_stdout_without_changing_sink() {
    let mut completed = 0;
    for verb in ["list", "status", "targets", "log", "sync", "doctor"] {
        for json in [false, true] {
            let root = tempfile::tempdir().expect("owned fixture");
            for name in ["home", "config", "data", "cache", "project", "tmp"] {
                fs::create_dir(root.path().join(name)).expect("fixture directory");
            }
            let sink = root.path().join("readonly");
            let sentinel = b"owned read-only stdout sentinel";
            fs::write(&sink, sentinel).expect("sentinel");
            let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
            command
                .env_clear()
                .envs(coverage::profile_environment())
                .env("HOME", root.path().join("home"))
                .env("USERPROFILE", root.path().join("home"))
                .env("XDG_CONFIG_HOME", root.path().join("config"))
                .env("XDG_DATA_HOME", root.path().join("data"))
                .env("XDG_CACHE_HOME", root.path().join("cache"))
                .env("TMPDIR", root.path().join("tmp"))
                .env("TEMP", root.path().join("tmp"))
                .env("TMP", root.path().join("tmp"))
                .env("PATH", "")
                .env("SYMBRAIN_GO_BINARY", root.path().join("never-go"))
                .current_dir(root.path().join("project"))
                .stdin(Stdio::null())
                .stdout(fs::File::open(&sink).expect("readonly handle"))
                .stderr(Stdio::piped())
                .args(["skills", verb]);
            for key in ["SystemRoot", "windir"] {
                if let Some(value) = std::env::var_os(key) {
                    command.env(key, value);
                }
            }
            if json {
                command.arg("--json");
            }
            let mut child = command.spawn().expect("actual CLI");
            let started = Instant::now();
            while child.try_wait().expect("child status").is_none() {
                if started.elapsed() > Duration::from_secs(20) {
                    child.kill().expect("stop timed-out CLI");
                    child.wait().expect("join timed-out CLI");
                    panic!("{verb}/{json}: readonly stdout probe timed out");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let output = child.wait_with_output().expect("completed CLI");
            assert_eq!(output.status.code(), Some(1), "{verb}/{json}: {output:?}");
            let message = String::from_utf8(output.stderr).expect("diagnostic");
            assert!(
                message.starts_with(&format!(
                    "symbrain skills {verb}: format output: write /dev/stdout:"
                )),
                "{verb}/{json}: {message}"
            );
            assert_eq!(fs::read(&sink).expect("sentinel readback"), sentinel);
            completed += 1;
        }
    }
    eprintln!("completed {completed} real readonly stdout cases");
}
