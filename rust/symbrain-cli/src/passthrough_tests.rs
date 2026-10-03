use super::*;

#[cfg(unix)]
fn executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, body).expect("write executable");
    let mut permissions = fs::metadata(path).expect("stat executable").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("chmod executable");
}

#[cfg(not(unix))]
fn executable(path: &Path, body: &str) {
    fs::write(path, body).expect("write executable");
}

#[test]
fn config_binary_path_is_read_from_nested_table() {
    let root = tempfile::tempdir().expect("tempdir");
    let config = root.path().join("config.toml");
    fs::write(
        &config,
        "[servers.vault]\nbinary_path = \"/custom/symvault\"\n",
    )
    .expect("write config");
    assert_eq!(
        crate::vault_config::configured_override(&config, "SYMBRAIN_TEST_UNUSED_VAULT_CONFIG"),
        Some(PathBuf::from("/custom/symvault"))
    );
}

#[test]
fn empty_config_binary_path_is_not_an_override() {
    let root = tempfile::tempdir().expect("tempdir");
    let config = root.path().join("config.toml");
    fs::write(&config, "[servers.vault]\nbinary_path = \"\"\n").expect("write config");
    assert_eq!(
        crate::vault_config::configured_override(&config, "SYMBRAIN_TEST_UNUSED_VAULT_CONFIG"),
        None
    );
}

#[cfg(unix)]
#[test]
fn configured_path_diagnostic_preserves_non_utf8_bytes() {
    use std::os::unix::ffi::OsStringExt;

    let path = PathBuf::from(OsString::from_vec(b"/tmp/missing_\xff".to_vec()));
    let mut stderr = Vec::new();
    write_configured_error(
        &mut stderr,
        "symbrain vault",
        &path,
        &std::io::Error::from(std::io::ErrorKind::NotFound),
    );
    assert!(
        stderr
            .windows(b"\"/tmp/missing_\\xff\"".len())
            .any(|bytes| { bytes == b"\"/tmp/missing_\\xff\"" })
    );
    assert!(
        stderr
            .windows(b"stat /tmp/missing_\xff:".len())
            .any(|bytes| { bytes == b"stat /tmp/missing_\xff:" })
    );
}

#[test]
fn managed_binary_requires_regular_executable_file() {
    let root = tempfile::tempdir().expect("tempdir");
    let binary = root.path().join(VAULT_BINARY);
    executable(&binary, "");
    assert!(is_executable_file(&binary));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&binary).expect("stat binary").permissions();
        permissions.set_mode(0o644);
        fs::set_permissions(&binary, permissions).expect("remove executable bit");
        assert!(!is_executable_file(&binary));
    }
}

#[test]
fn path_lookup_finds_binary_in_ordered_path_entry() {
    let root = tempfile::tempdir().expect("tempdir");
    let first = root.path().join("first");
    let second = root.path().join("second");
    fs::create_dir_all(&first).expect("first dir");
    fs::create_dir_all(&second).expect("second dir");
    #[cfg(windows)]
    let expected_binary = second.join(format!("{VAULT_BINARY}.exe"));
    #[cfg(not(windows))]
    let expected_binary = second.join(VAULT_BINARY);
    executable(&expected_binary, "");

    let found = path_lookup_in(OsStr::new(VAULT_BINARY), vec![first, second.clone()]);
    assert_eq!(found, Some(expected_binary.clone()));
    assert!(is_executable_file(&expected_binary));
}

#[test]
fn windows_path_extensions_match_go_defaults() {
    assert_eq!(
        windows_executable_names("symvault", Some("")),
        [
            "symvault.com",
            "symvault.exe",
            "symvault.bat",
            "symvault.cmd"
        ]
        .map(OsString::from)
    );
    assert_eq!(
        windows_executable_names("symvault.EXE", Some(".COM;.EXE")),
        vec![OsString::from("symvault.EXE")]
    );
    assert_eq!(
        windows_executable_names("symvault", Some("COM;Exe")),
        vec![
            OsString::from("symvault.com"),
            OsString::from("symvault.exe")
        ]
    );
}
