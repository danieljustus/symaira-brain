#![cfg(unix)]

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStringExt;
use std::process::{Command, Output};

fn run(binary: &OsStr, home: &OsStr, args: &[&str]) -> Output {
    Command::new(binary)
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("UNRELATED", OsString::from_vec(vec![0xff]))
        .env(OsString::from_vec(vec![0xfe]), "invalid-name")
        .output()
        .unwrap()
}

#[test]
fn harness_nonutf8_environment_preserves_table_bytes_and_replaces_json() {
    // No non-UTF-8 directory needs to exist (APFS cannot create one).
    let root = tempfile::tempdir().unwrap();
    let native = OsStr::new(env!("CARGO_BIN_EXE_symbrain"));
    // Optional fresh Go evidence, following config-harness-oracle's byte-table /
    // decoded-JSON comparisons. The regression assertions run without Go too.
    let oracle = std::env::var_os("SYMBRAIN_HARNESS_GO_ORACLE");
    for suffix in [
        b"/missing-\xff\xfe".as_slice(),
        b"/missing-\xe2\x82".as_slice(),
    ] {
        let mut home = root.path().as_os_str().to_os_string().into_vec();
        home.extend_from_slice(suffix);
        let home = OsString::from_vec(home);
        let table = run(native, &home, &["harness", "list"]);
        assert!(table.status.success(), "{table:?}");
        assert!(
            table
                .stdout
                .windows(suffix.len())
                .any(|part| part == suffix)
        );
        let json = run(native, &home, &["harness", "list", "--json"]);
        assert!(json.status.success(), "{json:?}");
        let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
        assert_eq!(
            value["harnesses"][0]["global"]["path"],
            format!(
                "{}/missing-\u{fffd}\u{fffd}/.claude.json",
                root.path().display()
            )
        );
        if let Some(oracle) = &oracle {
            let go_table = run(oracle, &home, &["harness", "list"]);
            assert_eq!(go_table.status.code(), table.status.code());
            assert_eq!(go_table.stderr, table.stderr);
            assert_eq!(go_table.stdout, table.stdout);
            let go_json = run(oracle, &home, &["harness", "list", "--json"]);
            assert_eq!(go_json.status.code(), json.status.code());
            assert_eq!(go_json.stderr, json.stderr);
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&go_json.stdout).unwrap(),
                value
            );
        }
        for args in [
            vec![
                "install",
                "--harness",
                "claude",
                "--profile",
                "test",
                "--dry-run",
            ],
            vec!["uninstall", "--harness", "claude", "--dry-run"],
        ] {
            let output = run(native, &home, &args);
            assert!(output.status.success(), "{args:?}: {output:?}");
            assert!(output.stderr.is_empty(), "{output:?}");
            if let Some(oracle) = &oracle {
                let go = run(oracle, &home, &args);
                assert_eq!(go.status.code(), output.status.code());
                assert_eq!(go.stdout, output.stdout);
                assert_eq!(go.stderr, output.stderr);
            }
        }
    }
    // Unrelated non-UTF-8 names and values must not break a valid HOME either.
    let valid = run(native, root.path().as_os_str(), &["harness", "list"]);
    assert!(valid.status.success(), "{valid:?}");
}
