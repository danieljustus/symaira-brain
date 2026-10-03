//! Unknown flag diagnostics preserve Go's unquoted operands.

use tempfile::TempDir;

#[path = "support/skills_preflight_process.rs"]
mod process;

const USAGE: &[u8] = b"\nUsage of skills sync:\n  -dry-run\n    \treport the plan without writing\n  -scope string\n    \tinstall scope: user or project (default \"user\")\n  -target string\n    \tlimit to one harness target\n";

#[test]
fn literal_unicode_flag_name_is_preserved() {
    let root = TempDir::new().unwrap();
    let output = process::run(&root, &["skills", "sync", "--bad\u{fffd}=value"], false);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        [
            "flag provided but not defined: -bad\u{fffd}".as_bytes(),
            USAGE
        ]
        .concat()
    );
}

#[cfg(unix)]
#[test]
fn raw_flag_names_and_bad_syntax_remain_byte_exact() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    for (flag, diagnostic) in [
        (
            b"--bad\xff".as_slice(),
            b"flag provided but not defined: -bad\xff".as_slice(),
        ),
        (
            b"--bad\xff=value",
            b"flag provided but not defined: -bad\xff",
        ),
        (b"-\xff", b"flag provided but not defined: -\xff"),
        (b"---\xff", b"flag provided but not defined: -\xff"),
        (b"--=\xff", b"bad flag syntax: -=\xff"),
        (b"----\xff", b"bad flag syntax: ---\xff"),
        (
            b"--bad\xc0\xaf",
            b"flag provided but not defined: -bad\xc0\xaf",
        ),
        (
            b"--bad\xe2\x82=value",
            b"flag provided but not defined: -bad\xe2\x82",
        ),
    ] {
        for prefix in [None, Some("--target=opencode"), Some("--dry-run=true")] {
            let root = TempDir::new().unwrap();
            let mut args = vec![OsString::from("skills"), OsString::from("sync")];
            args.extend(prefix.map(OsString::from));
            args.push(OsString::from_vec(flag.to_vec()));
            let output = process::run_os(&root, &args, false);
            assert_eq!(output.status.code(), Some(2), "{args:?}");
            assert!(output.stdout.is_empty(), "{args:?}");
            assert_eq!(output.stderr, [diagnostic, USAGE].concat(), "{args:?}");
            assert!(!root.path().join("data/symbrain/skills").exists());
        }
    }
}
