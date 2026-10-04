//! Prepared actual-process controls for the three independent source findings.
use std::process::{Command, Stdio};

fn main() {
    let binary = std::env::var_os("CONFIG13_CONTROL_NATIVE").expect("owned candidate");
    let mode = std::env::var("CONFIG13_CONTROL_MODE").unwrap();
    let mut command = Command::new(binary);
    command.args(std::env::args_os().skip(1));
    match mode.as_str() {
        "wrong-owner" => {
            command.env_remove("PWD");
        }
        "repair-codex" => {
            let profile = std::env::var_os("SYMBRAIN_DEFAULT_PROFILE").unwrap();
            let profile = symbrain_core::GoText::from(symbrain_core::go_path::os_bytes(&profile));
            command.env("SYMBRAIN_DEFAULT_PROFILE", profile.unicode_lossy());
        }
        "reject-prefix" => {
            let path = symbrain_core::config::resolved::default_path();
            let bytes = std::fs::read(path).unwrap();
            assert!(bytes.starts_with(b"\xff\xfe") || bytes.starts_with(b"\xfe\xff"));
            eprintln!("injected rejection of Go-admitted initial marker");
            std::process::exit(2);
        }
        _ => panic!("unknown owned control mode"),
    }
    let status = command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .expect("owned delegated candidate");
    std::process::exit(status.code().unwrap_or(1));
}
