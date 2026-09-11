use std::ffi::OsString;
use std::fs;

use tempfile::tempdir;

use super::*;

#[test]
fn set_accepts_go_single_dash_safeguard_flags() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "default_profile = \"personal\"\n").unwrap();
    let before = fs::read(&path).unwrap();

    let mut out = Vec::new();
    let mut err = Vec::new();
    assert_eq!(
        run_config_set_with_path(
            &path,
            &[
                OsString::from("-preview"),
                OsString::from("default_profile"),
                OsString::from("secret-value"),
            ],
            &mut out,
            &mut err,
        ),
        crate::exit::OK
    );
    assert!(String::from_utf8_lossy(&out).contains("preview config set default_profile: changed"));
    assert!(!String::from_utf8_lossy(&out).contains("secret-value"));
    assert_eq!(fs::read(&path).unwrap(), before);

    out.clear();
    err.clear();
    assert_eq!(
        run_config_set_with_path(
            &path,
            &[
                OsString::from("-no-backup"),
                OsString::from("default_profile"),
                OsString::from("restricted"),
            ],
            &mut out,
            &mut err,
        ),
        crate::exit::OK
    );
    assert!(!path.with_file_name("config.toml.bak").exists());
}
