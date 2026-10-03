use super::*;

#[test]
fn origin_identity_preserves_each_invalid_byte() {
    for (raw, expected) in [
        (b"work\xe2\x82".as_slice(), "work\u{fffd}\u{fffd}"),
        (b"work\xff", "work\u{fffd}"),
        (b"work\xc0\xaf", "work\u{fffd}\u{fffd}"),
        (b"work\xef\xbf\xbd", "work\u{fffd}"),
        (b"work\xed\xa0\x80", "work\u{fffd}\u{fffd}\u{fffd}"),
    ] {
        assert_eq!(go_json_text(raw), expected);
    }
}

#[test]
fn cache_environment_uses_go_platform_fallbacks_in_isolated_children() {
    let temp = std::env::temp_dir();
    let home = temp.join("cache-path-home");
    let cache = temp.join("cache-path-xdg");
    let fallback = temp.join("symbrowse").join("sessions");
    let home_cache = if cfg!(target_os = "macos") {
        home.join("Library/Caches/symbrowse/sessions")
    } else {
        home.join(".cache/symbrowse/sessions")
    };
    for kind in [
        "missing",
        "empty",
        "home",
        "absolute-cache",
        "relative-cache",
        "missing-temp",
        "empty-temp",
    ] {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "spec::paths::tests::cache_environment_child",
                "--ignored",
            ])
            .env_remove("HOME")
            .env_remove("LOCALAPPDATA")
            .env_remove("XDG_CACHE_HOME");
        let expected = match kind {
            "missing" => fallback.clone(),
            "empty" => {
                command
                    .env("HOME", "")
                    .env("LOCALAPPDATA", "")
                    .env("XDG_CACHE_HOME", "");
                fallback.clone()
            }
            "missing-temp" | "empty-temp" => {
                if kind == "missing-temp" {
                    command.env_remove("TMPDIR");
                } else {
                    command.env("TMPDIR", "");
                }
                if cfg!(unix) {
                    PathBuf::from("/tmp/symbrowse/sessions")
                } else {
                    fallback.clone()
                }
            }
            "home" => {
                command.env("HOME", &home).env("LOCALAPPDATA", &home);
                if cfg!(windows) {
                    home.join("symbrowse").join("sessions")
                } else {
                    home_cache.clone()
                }
            }
            "absolute-cache" => {
                command
                    .env("HOME", &home)
                    .env("LOCALAPPDATA", &cache)
                    .env("XDG_CACHE_HOME", &cache);
                if cfg!(target_os = "macos") {
                    home_cache.clone()
                } else {
                    cache.join("symbrowse").join("sessions")
                }
            }
            "relative-cache" => {
                command
                    .env("HOME", &home)
                    .env("LOCALAPPDATA", &home)
                    .env("XDG_CACHE_HOME", "relative-cache");
                if cfg!(windows) {
                    home.join("symbrowse").join("sessions")
                } else if cfg!(target_os = "macos") {
                    home_cache.clone()
                } else {
                    fallback.clone()
                }
            }
            _ => unreachable!(),
        };
        let output = command
            .env("SYMBROWSE_CACHE_PATH_TEST_EXPECTED", expected)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{kind}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "child did not execute"
        );
    }
}

#[test]
#[ignore = "isolated environment child explicitly executed by its parent"]
fn cache_environment_child() {
    let expected = PathBuf::from(std::env::var_os("SYMBROWSE_CACHE_PATH_TEST_EXPECTED").unwrap());
    assert_eq!(default_session_cache_root(), expected);
}
