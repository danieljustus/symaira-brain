#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn harness_registry_nonutf8_environment() {
    // Isolate process environment changes from the parallel test runner.
    if std::env::var_os("HARNESS_NONUTF8_CHILD").is_some() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let xdg = PathBuf::from(std::env::var_os("XDG_CONFIG_HOME").unwrap());
        for (name, root, relative) in [
            ("claude", &home, ".claude.json"),
            ("opencode", &xdg, "opencode/config.json"),
        ] {
            let harness = symbrain_harness::lookup(name).unwrap();
            let expected = root.join(relative);
            assert_eq!(harness.config_path().unwrap(), expected);
            let location = harness.config_location().unwrap();
            assert_eq!(location.path, expected);
            assert_eq!(location.trusted_root, *root);
            assert_eq!(location.relative_path, PathBuf::from(relative));
        }
        let inventory = symbrain_harness::list(None);
        assert_eq!(
            inventory.harnesses[0].global.raw_path,
            home.join(".claude.json")
        );
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let mut path = root.path().as_os_str().to_os_string().into_vec();
    path.extend_from_slice(b"/missing-\xff");
    let path = OsString::from_vec(path);
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "harness_registry_nonutf8_environment",
            "--nocapture",
        ])
        .env_clear()
        .env("HARNESS_NONUTF8_CHILD", "1")
        .env("HOME", &path)
        .env("XDG_CONFIG_HOME", &path)
        .env("UNRELATED", OsString::from_vec(vec![0xfe]))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}
