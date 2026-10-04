//! Real native processes retain lone UTF-16 surrogates in skills diagnostics.

#![cfg(windows)]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use tempfile::TempDir;

#[path = "support/skills_preflight_process.rs"]
mod process;

#[test]
fn native_wide_flag_operands_do_not_become_replacement_characters() {
    ascii_preflight_bootstrap();
    for (prefix, suffix, diagnostic) in [
        (
            "--bad",
            "=value",
            b"flag provided but not defined: -bad\xed\xa0\x80".as_slice(),
        ),
        ("-", "", b"flag provided but not defined: -\xed\xa0\x80"),
        ("----", "", b"bad flag syntax: ---\xed\xa0\x80"),
        ("--=", "", b"bad flag syntax: -=\xed\xa0\x80"),
    ] {
        let root = TempDir::new().unwrap();
        let mut wide: Vec<_> = prefix.encode_utf16().collect();
        wide.push(0xd800);
        wide.extend(suffix.encode_utf16());
        let output = process::run_os(
            &root,
            &["skills".into(), "sync".into(), OsString::from_wide(&wide)],
            false,
        );
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(output.stderr.starts_with(diagnostic));
        assert_eq!(output.stderr[diagnostic.len()], b'\n');
        assert!(!root.path().join("data/symbrain/skills").exists());
    }
}

#[test]
fn native_wide_values_quote_original_bytes_and_trim_target_only() {
    ascii_preflight_bootstrap();
    for (flag, padded, expected) in [
        ("--target=", true, "unknown target \"\\xed\\xa0\\x80\""),
        ("--scope=", true, "unknown scope \" \\xed\\xa0\\x80 \""),
        (
            "--dry-run=",
            false,
            "invalid boolean value \"\\xed\\xa0\\x80\"",
        ),
    ] {
        let root = TempDir::new().unwrap();
        let mut wide: Vec<_> = flag.encode_utf16().collect();
        if padded {
            wide.push(0x20);
        }
        wide.push(0xd800);
        if padded {
            wide.push(0x20);
        }
        let output = process::run_os(
            &root,
            &["skills".into(), "sync".into(), OsString::from_wide(&wide)],
            false,
        );
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr).unwrap().contains(expected));
        assert!(!root.path().join("data/symbrain/skills").exists());
    }
}

fn ascii_preflight_bootstrap() {
    let root = TempDir::new().unwrap();
    let output = process::run(
        &root,
        &["skills", "sync", "--bootstrap-invalid=value"],
        false,
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"flag provided but not defined: -bootstrap-invalid\nUsage of skills sync:\n  -dry-run\n    \treport the plan without writing\n  -scope string\n    \tinstall scope: user or project (default \"user\")\n  -target string\n    \tlimit to one harness target\n"
    );
    assert!(!root.path().join("data/symbrain/skills").exists());
}
