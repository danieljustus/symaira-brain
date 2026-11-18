//! `symbrain browse|operate|scope` run the bundled worker with inherited stdio.
//!
//! Every test fakes the worker with a shell script in a disposable directory;
//! none assumes a real Symaira binary is installed.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const FAKE_WORKER: &str = "#!/bin/sh\n\
printf '%s\\n' \"$@\" > \"$FAKE_WORKER_LOG\"\n\
printf 'stdin:'\n\
/bin/cat\n\
printf 'worker-stderr\\n' >&2\n\
exit \"${FAKE_WORKER_EXIT:-0}\"\n";

fn install_fake(directory: &Path, name: &str) {
    fs::create_dir_all(directory).unwrap();
    let path = directory.join(name);
    fs::write(&path, FAKE_WORKER).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn symbrain(root: &Path, path_dir: &Path, args: &[&str], stdin: &str, exit: i32) -> Output {
    fs::create_dir_all(root.join("home")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_symbrain"))
        .args(args)
        .env_clear()
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("PATH", path_dir)
        .env("FAKE_WORKER_LOG", root.join("args.log"))
        .env("FAKE_WORKER_EXIT", exit.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn browse_forwards_arguments_stdio_and_exit_code_to_symbrowse() {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    install_fake(&bin, "symbrowse");
    let output = symbrain(
        root.path(),
        &bin,
        &["browse", "fetch", "--json", "-x", "a b"],
        "payload",
        7,
    );
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "stdin:payload");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "worker-stderr\n");
    assert_eq!(
        fs::read_to_string(root.path().join("args.log")).unwrap(),
        "fetch\n--json\n-x\na b\n"
    );
}

#[test]
fn help_flags_after_the_command_belong_to_the_worker() {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    install_fake(&bin, "symbrowse");
    let output = symbrain(root.path(), &bin, &["browse", "--help"], "", 0);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        fs::read_to_string(root.path().join("args.log")).unwrap(),
        "--help\n"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn operate_and_scope_run_their_own_workers_on_macos() {
    for (command, worker) in [("operate", "symoperate"), ("scope", "symscope")] {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        install_fake(&bin, worker);
        let output = symbrain(root.path(), &bin, &[command, "status"], "", 3);
        assert_eq!(output.status.code(), Some(3), "{command}");
        assert_eq!(
            fs::read_to_string(root.path().join("args.log")).unwrap(),
            "status\n",
            "{command}"
        );
    }
}

#[test]
fn missing_worker_is_one_stderr_line_and_exit_one() {
    for (command, worker) in [
        ("browse", "symbrowse"),
        #[cfg(target_os = "macos")]
        ("operate", "symoperate"),
        #[cfg(target_os = "macos")]
        ("scope", "symscope"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let empty = root.path().join("empty-bin");
        fs::create_dir_all(&empty).unwrap();
        let output = symbrain(root.path(), &empty, &[command, "anything"], "", 0);
        assert_eq!(output.status.code(), Some(1), "{command}");
        assert!(output.stdout.is_empty(), "{command}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(stderr.lines().count(), 1, "{command}: {stderr}");
        assert!(stderr.contains(worker), "{command}: {stderr}");
        assert!(
            stderr.contains("ships with symbrain since the bundled release"),
            "{command}: {stderr}"
        );
        assert!(!root.path().join("args.log").exists());
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn operate_and_scope_are_unavailable_off_macos_even_with_a_worker_present() {
    for (command, worker) in [("operate", "symoperate"), ("scope", "symscope")] {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        install_fake(&bin, worker);
        let output = symbrain(root.path(), &bin, &[command], "", 0);
        assert_eq!(output.status.code(), Some(1), "{command}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(stderr.lines().count(), 1, "{command}: {stderr}");
        assert!(stderr.contains("unavailable on this platform"), "{stderr}");
        assert!(!root.path().join("args.log").exists());
    }
}

#[test]
fn worker_next_to_the_symbrain_executable_wins_over_an_empty_path() {
    let root = tempfile::tempdir().unwrap();
    let bundle = root.path().join("bundle");
    fs::create_dir_all(&bundle).unwrap();
    // Copy the real binary so `current_exe()` resolves into the bundle directory.
    let relocated = bundle.join("symbrain");
    fs::copy(env!("CARGO_BIN_EXE_symbrain"), &relocated).unwrap();
    install_fake(&bundle, "symbrowse");
    let empty = root.path().join("empty-bin");
    fs::create_dir_all(&empty).unwrap();
    fs::create_dir_all(root.path().join("home")).unwrap();
    let status = Command::new(&relocated)
        .args(["browse", "hello"])
        .env_clear()
        .env("HOME", root.path().join("home"))
        .env("PATH", &empty)
        .env("FAKE_WORKER_LOG", root.path().join("args.log"))
        .env("FAKE_WORKER_EXIT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0));
    assert_eq!(
        fs::read_to_string(root.path().join("args.log")).unwrap(),
        "hello\n"
    );
}
