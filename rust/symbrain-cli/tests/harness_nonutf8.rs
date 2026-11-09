#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStringExt;
use std::process::{Command, Output};

fn run(binary: &OsStr, home: &OsStr, args: &[&str]) -> Output {
    Command::new(binary)
        .args(args)
        .env_clear()
        .envs(coverage::profile_environment())
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
    // Go parity for the byte table / decoded JSON is frozen in
    // fixtures/config_harness_go_shapes.json (config-harness-oracle); this test pins the
    // non-UTF-8 regression behavior natively.
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
        #[cfg(target_os = "macos")]
        {
            replay_go(&["harness", "list"], &table, root.path(), suffix);
            replay_go(&["harness", "list", "--json"], &json, root.path(), suffix);
        }
        let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
        assert_eq!(
            value["harnesses"][0]["global"]["path"],
            format!(
                "{}/missing-\u{fffd}\u{fffd}/.claude.json",
                root.path().display()
            )
        );
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
            #[cfg(target_os = "macos")]
            replay_go(&args, &output, root.path(), suffix);
        }
    }
    // Unrelated non-UTF-8 names and values must not break a valid HOME either.
    let valid = run(native, root.path().as_os_str(), &["harness", "list"]);
    assert!(valid.status.success(), "{valid:?}");
}

/// The frozen Go transcript (recorded on darwin; other platforms keep the native assertions
/// above because the harness inventory differs per OS).
#[cfg(target_os = "macos")]
fn replay_go(argv: &[&str], observed: &Output, root: &std::path::Path, suffix: &[u8]) {
    use std::os::unix::ffi::OsStrExt;
    fn unhex(value: &str) -> Vec<u8> {
        (0..value.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
            .collect()
    }
    fn tokenize(bytes: &[u8], root: &std::path::Path) -> Vec<u8> {
        let needle = root.as_os_str().as_bytes();
        let mut out = Vec::new();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index..].starts_with(needle) {
                out.extend_from_slice(b"<ROOT>");
                index += needle.len();
            } else {
                out.push(bytes[index]);
                index += 1;
            }
        }
        out
    }
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/harness_nonutf8_go.darwin.json")).unwrap();
    let suffix_hex = suffix.iter().fold(String::new(), |mut out, b| {
        use std::fmt::Write;
        write!(out, "{b:02x}").unwrap();
        out
    });
    let case = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| {
            case["suffix_hex"] == suffix_hex.as_str() && case["argv"] == serde_json::json!(argv)
        })
        .unwrap_or_else(|| panic!("no frozen Go transcript for {argv:?} {suffix_hex}"));
    assert_eq!(case["exit"], observed.status.code().unwrap(), "{argv:?}");
    assert_eq!(
        unhex(case["stderr_hex"].as_str().unwrap()),
        tokenize(&observed.stderr, root),
        "{argv:?} stderr"
    );
    let go_stdout = unhex(case["stdout_hex"].as_str().unwrap());
    let native_stdout = tokenize(&observed.stdout, root);
    if argv[0] == "harness" && argv.contains(&"--json") {
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&go_stdout).unwrap(),
            serde_json::from_slice::<serde_json::Value>(&native_stdout).unwrap(),
            "{argv:?}"
        );
    } else {
        assert_eq!(
            go_stdout, native_stdout,
            "{argv:?} stdout bytes differ from frozen Go"
        );
    }
}
