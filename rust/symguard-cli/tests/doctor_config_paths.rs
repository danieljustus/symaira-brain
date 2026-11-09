//! Exact Brain adapter: generated config owner bytes for every doctor state.
use std::{fs, process::Command};
#[path = "../../symbrain-cli/src/guard_cli.rs"]
#[expect(
    unused_imports,
    reason = "This test uses the exact Brain run adapter; run_at_path has other consumers"
)]
mod brain_adapter;

#[test]
fn generated_config_owner_is_shared_by_brain() {
    if let Ok(role) = std::env::var("OWNED_GUARD_CONFIG_PATH_ROLE") {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let result = brain_adapter::run(&["doctor".into()], &mut stdout, &mut stderr);
        if role == "typed" {
            // Go stops at the decoder error, before any undecoded-key warning.
            assert_eq!(result, Some(1), "stdout={stdout:?}, stderr={stderr:?}");
            assert!(stdout.starts_with(b"symguard doctor\n"));
            assert!(stderr.is_empty());
        } else {
            assert_eq!(
                result,
                Some(u8::from(matches!(role.as_str(), "semantic" | "discovery"))),
                "role {role}: stdout={stdout:?}, stderr={stderr:?}"
            );
            assert!(stdout.starts_with(b"symguard doctor\n"));
            let expected = std::env::var_os("OWNED_EXPECTED_CONFIG_PATH").unwrap();
            let warning = symbrain_core::GoText::path(
                "config: warning: unknown key \"lexical_owner\" in ",
                std::path::Path::new(&expected),
                "\n",
            );
            // Exact owner bytes, including generated/native versus explicit separators.
            assert_eq!(stderr, warning.as_ref(), "role {role}: warning owner bytes");
            assert!(!stderr.windows(2).any(|part| part == b".."));
        }
        return;
    }
    let roles = ["healthy", "semantic", "typed", "discovery"].into_iter();
    for role in roles {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        // A slash inside one PathBuf component survives verbatim on Windows.
        // The generated Go owner uses native separators, not that mixed spelling.
        let config = root
            .path()
            .join("config")
            .join("symguard")
            .join("config.toml");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::create_dir_all(root.path().join("discard")).unwrap();
        fs::create_dir_all(&home).unwrap();
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
        let xdg = root.path().join("discard").join("..").join("config");
        let expected_config = config.clone();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "generated_config_owner_is_shared_by_brain",
                "--nocapture",
            ])
            .env_clear()
            .env("OWNED_GUARD_CONFIG_PATH_ROLE", role)
            .env("OWNED_EXPECTED_CONFIG_PATH", &expected_config)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("XDG_CONFIG_HOME", &xdg)
            .env("XDG_DATA_HOME", root.path().join("data"))
            .env("XDG_CACHE_HOME", root.path().join("cache"))
            .env("PATH", "")
            .current_dir(root.path());
        if role == "healthy-slash-override" {
            command.env("SYMGUARD_CONFIG", &expected_config);
        }
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
