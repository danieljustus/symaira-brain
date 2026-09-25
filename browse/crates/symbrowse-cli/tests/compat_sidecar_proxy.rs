#![cfg(unix)]

use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Stdio},
};
use tempfile::tempdir;

fn run(binary: &Path, helper: Option<&Path>, input: &[u8]) -> std::process::Output {
    let mut command = Command::new(binary);
    command.arg("compat-sidecar");
    if let Some(helper) = helper {
        command.env("SYMBROWSE_COMPAT_BINARY", helper);
    } else {
        command.env_remove("SYMBROWSE_COMPAT_BINARY");
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start symbrowse compat-sidecar");
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(input)
        .expect("write sidecar protocol input");
    child.wait_with_output().expect("wait for sidecar proxy")
}

#[test]
fn compat_sidecar_forwards_protocol_stdio_to_retained_helper() {
    let temp = tempdir().expect("temporary helper dir");
    let helper = temp.path().join("go-sidecar-stub");
    fs::write(
        &helper,
        "#!/bin/sh\n[ \"$1\" = compat-sidecar ] || exit 73\ncat\n",
    )
    .expect("write helper stub");
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o700))
        .expect("make helper executable");

    let input = b"{\"type\":\"handshake\",\"protocol\":1}\n";
    let output = run(
        Path::new(env!("CARGO_BIN_EXE_symbrowse")),
        Some(&helper),
        input,
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, input);
    assert!(output.stderr.is_empty());
}

#[test]
fn compat_sidecar_fails_closed_without_helper_or_on_self_recursion() {
    let binary = Path::new(env!("CARGO_BIN_EXE_symbrowse"));
    let missing = run(binary, None, b"");
    assert_eq!(missing.status.code(), Some(1));
    assert!(missing.stdout.is_empty());
    assert_eq!(
        missing.stderr,
        b"compat-sidecar requires SYMBROWSE_COMPAT_BINARY to name the retained Go helper\n"
    );

    let relative = Path::new("relative-sidecar-helper");
    let relative_helper = run(binary, Some(relative), b"");
    assert_eq!(relative_helper.status.code(), Some(1));
    assert!(relative_helper.stdout.is_empty());
    assert_eq!(
        relative_helper.stderr,
        b"compat-sidecar requires SYMBROWSE_COMPAT_BINARY to be an absolute path\n"
    );

    let self_helper = run(binary, Some(binary), b"");
    assert_eq!(self_helper.status.code(), Some(1));
    assert!(self_helper.stdout.is_empty());
    assert_eq!(
        self_helper.stderr,
        b"compat-sidecar cannot use the symbrowse Rust executable as its helper\n"
    );
}
