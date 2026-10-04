//! Exact Brain adapter: generated config owner and empty-stream delegation.
use std::{fs, process::Command};
#[path = "../../symbrain-cli/src/guard_cli.rs"]
#[expect(
    unused_imports,
    reason = "This test uses the exact Brain run adapter; run_at_path has other consumers"
)]
mod brain_adapter;
#[cfg(windows)]
#[path = "support/windows_discovery.rs"]
mod windows_discovery;

#[test]
fn generated_owner_and_delegation_are_shared_by_brain() {
    if let Ok(role) = std::env::var("OWNED_GUARD_CONFIG_PATH_ROLE") {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let result = brain_adapter::run(&["doctor".into()], &mut stdout, &mut stderr);
        if role == "typed" || role == "discovery" {
            assert_eq!(result, None);
            assert!(stdout.is_empty() && stderr.is_empty());
        } else {
            assert_eq!(
                result,
                Some(u8::from(role == "semantic")),
                "role {role}: stdout={stdout:?}, stderr={stderr:?}"
            );
            assert!(stdout.starts_with(b"symguard doctor\n"));
            assert!(stderr.starts_with(b"config: warning: unknown key \"lexical_owner\" in "));
            let expected = std::env::var_os("OWNED_EXPECTED_CONFIG_PATH").unwrap();
            let path = symbrain_core::GoText::path("", std::path::Path::new(&expected), "");
            // Report and warnings must use the lexically generated file name.
            assert!(
                stderr
                    .windows(path.as_ref().len())
                    .any(|part| part == path.as_ref())
            );
            assert!(!stderr.windows(2).any(|part| part == b".."));
        }
        return;
    }
    for role in ["healthy", "semantic", "typed", "discovery"] {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let config = root.path().join("config/symguard/config.toml");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::create_dir_all(root.path().join("discard")).unwrap();
        fs::create_dir_all(&home).unwrap();
        #[cfg(windows)]
        windows_discovery::create_parents(&home, &root.path().join("config"));
        let text = match role {
            "semantic" => "lexical_owner=1\nsequence={enabled=true,threshold=1}\n",
            "typed" => "lexical_owner=1\nsequence={enabled=\"bad\"}\n",
            _ => "lexical_owner=1\n",
        };
        fs::write(&config, text).unwrap();
        if role == "discovery" {
            fs::create_dir_all(home.join(".cursor")).unwrap();
            fs::write(home.join(".cursor/mcp.json"), "{bad").unwrap();
        }
        let before = fs::metadata(&config).unwrap().modified().unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "generated_owner_and_delegation_are_shared_by_brain",
                "--nocapture",
            ])
            .env_clear()
            .env("OWNED_GUARD_CONFIG_PATH_ROLE", role)
            .env("OWNED_EXPECTED_CONFIG_PATH", &config)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env(
                "XDG_CONFIG_HOME",
                root.path().join("discard").join("..").join("config"),
            )
            .env("XDG_DATA_HOME", root.path().join("data"))
            .env("XDG_CACHE_HOME", root.path().join("cache"))
            .env("PATH", "")
            .current_dir(root.path());
        for key in ["SystemRoot", "WINDIR", "TMP", "TEMP"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let child = command.output().unwrap();
        assert!(child.status.success(), "{role}: {child:?}");
        assert_eq!(fs::read(&config).unwrap(), text.as_bytes());
        assert_eq!(fs::metadata(&config).unwrap().modified().unwrap(), before);
    }
}

#[cfg(windows)]
#[test]
fn windows_discovery_file_and_parent_errors_keep_doctor_contract() {
    if std::env::var_os("OWNED_GUARD_WINDOWS_DISCOVERY").is_some() {
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
        let first_source = home.join(".config/hermes/config.json");
        let config = std::path::PathBuf::from(std::env::var_os("SYMGUARD_CONFIG").unwrap());
        let original = fs::read(&config).unwrap();
        let modified = fs::metadata(&config).unwrap().modified().unwrap();
        assert_eq!(fs::read(&first_source).unwrap_err().raw_os_error(), Some(2));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            brain_adapter::run(&["doctor".into()], &mut stdout, &mut stderr),
            Some(0),
            "existing discovery parents: stdout={stdout:?}, stderr={stderr:?}"
        );
        println!("WINDOWS_GUARD_DISCOVERY error=2 stdout={stdout:?} stderr={stderr:?}");
        assert!(
            stdout
                .windows(23)
                .any(|part| part == b"All basic checks passed")
        );

        fs::remove_dir(first_source.parent().unwrap()).unwrap();
        assert_eq!(fs::read(&first_source).unwrap_err().raw_os_error(), Some(3));
        stdout.clear();
        stderr.clear();
        assert_eq!(
            brain_adapter::run(&["doctor".into()], &mut stdout, &mut stderr),
            Some(1),
            "missing discovery parent: stdout={stdout:?}, stderr={stderr:?}"
        );
        println!("WINDOWS_GUARD_DISCOVERY error=3 stdout={stdout:?} stderr={stderr:?}");
        assert!(stdout.windows(17).any(|part| part == b"discovery: hermes"));
        assert!(stdout.windows(16).any(|part| part == b"1 issue(s) found"));
        assert_eq!(fs::read(&config).unwrap(), original);
        assert_eq!(fs::metadata(&config).unwrap().modified().unwrap(), modified);
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let xdg = root.path().join("config");
    windows_discovery::create_parents(&home, &xdg);
    let config = xdg.join("symguard/config.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, "owned=1\n").unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "windows_discovery_file_and_parent_errors_keep_doctor_contract",
            "--nocapture",
        ])
        .env_clear()
        .env("OWNED_GUARD_WINDOWS_DISCOVERY", "1")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", &xdg)
        .env("XDG_DATA_HOME", root.path().join("data"))
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .env("SYMGUARD_CONFIG", &config)
        .env("PATH", "")
        .current_dir(root.path());
    for key in ["SystemRoot", "WINDIR", "TMP", "TEMP"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let child = command.output().unwrap();
    println!("WINDOWS_GUARD_DISCOVERY_CHILD {child:?}");
    assert!(child.status.success(), "{child:?}");
}

#[cfg(windows)]
#[test]
fn raw_windows_config_path_delegates_before_output() {
    use std::os::windows::ffi::OsStringExt;
    if std::env::var_os("OWNED_RAW_WINDOWS_CONFIG").is_some() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            brain_adapter::run(&["doctor".into()], &mut stdout, &mut stderr),
            None
        );
        assert!(stdout.is_empty() && stderr.is_empty());
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join(std::ffi::OsString::from_wide(&[0xd800]));
    let config = base.join("symguard/config.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(&config, "owned=1\n").unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "raw_windows_config_path_delegates_before_output",
            "--nocapture",
        ])
        .env_clear()
        .env("OWNED_RAW_WINDOWS_CONFIG", "1")
        .env("HOME", root.path())
        .env("USERPROFILE", root.path())
        .env("XDG_CONFIG_HOME", &base)
        .env("PATH", "")
        .current_dir(root.path());
    for key in ["SystemRoot", "WINDIR", "TMP", "TEMP"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let child = command.output().unwrap();
    assert!(child.status.success(), "{child:?}");
    assert_eq!(fs::read(&config).unwrap(), b"owned=1\n");
}
