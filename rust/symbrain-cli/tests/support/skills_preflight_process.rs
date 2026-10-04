//! Isolated native preflight process with bounded execution.

use std::ffi::OsString;
use std::fs;
use std::io::{Read, Seek};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use tempfile::TempDir;

pub fn run(root: &TempDir, args: &[&str], configured: bool) -> Output {
    let args: Vec<_> = args.iter().map(OsString::from).collect();
    run_os(root, &args, configured)
}

pub fn run_os(root: &TempDir, args: &[OsString], configured: bool) -> Output {
    let home = root.path().join("home");
    let config = root.path().join("config");
    let data = root.path().join("data");
    let project = root.path().join("project");
    for path in [&home, &config, &data, &project] {
        fs::create_dir_all(path).unwrap();
    }
    let stdout = tempfile::tempfile_in(root.path()).unwrap();
    let stderr = tempfile::tempfile_in(root.path()).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .env_clear()
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_DATA_HOME", &data)
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("TZ", "UTC")
        .current_dir(project)
        .stdout(Stdio::from(stdout.try_clone().unwrap()))
        .stderr(Stdio::from(stderr.try_clone().unwrap()))
        .args(args);
    if configured {
        command.env(
            "SYMBRAIN_SKILLS_LIBRARY_DIR",
            root.path().join("other-library"),
        );
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("native preflight hung for {args:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut stdout = stdout;
    let mut stderr = stderr;
    stdout.rewind().unwrap();
    stderr.rewind().unwrap();
    let mut output = Output {
        status,
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    stdout.read_to_end(&mut output.stdout).unwrap();
    stderr.read_to_end(&mut output.stderr).unwrap();
    output
}
