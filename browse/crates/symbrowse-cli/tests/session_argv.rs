//! Raw argv bytes and root-prefix `--session` spelling (#772 DMN-007/008).
//!
//! Expectations are bytes of the sealed Go recording
//! `migration/evidence/browse-state-key-772/candidate-4cd`, replayed in full by
//! `port/harness/daemon_registry_replay.py`; these cases keep the two behaviors
//! that regressed once under `cargo test`.
#![deny(unsafe_code)]
#![cfg(unix)]

use std::{
    ffi::OsString,
    os::unix::ffi::OsStringExt,
    process::{Command, Output},
};

fn run(args: &[OsString]) -> (Output, tempfile::TempDir) {
    let home = tempfile::tempdir().expect("private home");
    let output = Command::new(env!("CARGO_BIN_EXE_symbrowse"))
        .args(args)
        .current_dir(home.path())
        .env_clear()
        .env("HOME", home.path())
        .env("XDG_RUNTIME_DIR", home.path().join("run"))
        .env("SYMBROWSE_NO_AUTOSTART", "1")
        .env("SYMAIRA_CUTOVER_BACKUP", "off")
        .output()
        .expect("run symbrowse");
    (output, home)
}

fn raw(parts: &[&[u8]]) -> Vec<OsString> {
    parts
        .iter()
        .map(|part| OsString::from_vec(part.to_vec()))
        .collect()
}

#[test]
fn malformed_utf8_session_is_quoted_byte_for_byte() {
    for (name, quoted) in [
        (&b"bad\xffsession"[..], "bad\\xffsession"),
        (&b"bad\xe2\x82session"[..], "bad\\xe2\\x82session"),
        (&b"bad\xc0\xafsession"[..], "bad\\xc0\\xafsession"),
    ] {
        let expected =
            format!("invalid session \"{quoted}\": use 1-64 letters, digits, '.', '_' or '-'\n");
        for args in [
            raw(&[b"session", b"list", b"--session", name]),
            raw(&[
                b"session",
                b"list",
                &[b"--session=".as_slice(), name].concat(),
            ]),
            raw(&[b"state", b"list", b"--session", name]),
        ] {
            let (output, home) = run(&args);
            assert_eq!(output.status.code(), Some(1), "args={args:?}");
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                expected,
                "args={args:?}"
            );
            assert!(output.stdout.is_empty(), "args={args:?}");
            assert_eq!(
                std::fs::read_dir(home.path()).expect("home").count(),
                0,
                "an invalid session must not create files: args={args:?}"
            );
        }
    }
}

#[test]
fn replacement_scalar_in_session_stays_a_real_scalar() {
    let (output, _home) = run(&raw(&[
        b"session",
        b"list",
        b"--session",
        "bad\u{fffd}session".as_bytes(),
    ]));
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "invalid session \"bad\u{fffd}session\": use 1-64 letters, digits, '.', '_' or '-'\n"
    );
}

#[test]
fn root_prefix_session_reaches_the_daemon_client() {
    for group in [
        ["session", "list"],
        ["session", "info"],
        ["daemon", "status"],
        ["state", "list"],
    ] {
        let args = raw(&[
            b"--session",
            b"owned-prefix",
            group[0].as_bytes(),
            group[1].as_bytes(),
        ]);
        let (output, _home) = run(&args);
        assert_eq!(output.status.code(), Some(1), "args={args:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "daemon is unavailable for session \"owned-prefix\"\n",
            "args={args:?}"
        );
    }
}

#[test]
fn root_prefix_session_is_rejected_for_unrelated_commands() {
    for group in [&["version"][..], &["config", "show"], &["profiles"]] {
        let mut parts: Vec<&[u8]> = vec![b"--session=alpha"];
        parts.extend(group.iter().map(|word| word.as_bytes()));
        let (output, _home) = run(&raw(&parts));
        assert_eq!(output.status.code(), Some(1), "group={group:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "unknown flag: --session\n",
            "group={group:?}"
        );
    }
}
