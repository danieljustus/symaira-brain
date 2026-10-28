//! Opt-in backends remain native and never inspect excluded credentials.
#[cfg(unix)]
#[test]
fn opt_in_backends_validate_before_discovery() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    let home = tempfile::tempdir().unwrap();
    let marker = home.path().join("accessed");
    for name in ["symvault", "security", "ps", "lsof", "gh", "go-fallback"] {
        let path = home.path().join(name);
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf accessed >> '{}'\nexit 1\n",
                marker.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let run = |selector: &str, poison: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
        command
            .args(["usage", "--json", "--providers", selector])
            .env_clear()
            .env("HOME", home.path())
            .env("GROK_HOME", home.path())
            .env("PATH", home.path())
            .env("SYMBRAIN_GO_BINARY", home.path().join("go-fallback"));
        for key in [
            "ANTHROPIC_OAUTH_TOKEN",
            "COPILOT_ACCESS_TOKEN",
            "KIMI_CODE_API_KEY",
            "NOUS_PORTAL_ACCESS_TOKEN",
            "OPENCODE_COOKIE",
        ] {
            command.env(key, "symvault://test/excluded");
        }
        if poison {
            for key in ["ZAI_API_KEY", "DEVIN_API_KEY", "GROK_ACCESS_TOKEN"] {
                command.env(key, "symvault://test/must-not-resolve");
            }
        }
        command.output().unwrap()
    };
    for id in ["zai", "devin", "grok", "ollama"] {
        let result = run(id, false);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        let providers = report["providers"].as_array().unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0]["id"], id);
        if id == "ollama" {
            assert_eq!(providers[0]["configured"], true);
            assert!(
                providers[0]["error"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())
            );
        } else {
            assert_eq!(providers[0]["configured"], false);
        }
        let result = run(&format!("{id},unknown"), true);
        assert_eq!(result.status.code(), Some(2));
        assert!(result.stdout.is_empty());
    }
    let result = run("ollama,grok,devin,zai,zai", false);
    assert!(result.status.success());
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        report["providers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["zai", "devin", "grok", "ollama"]
    );
    assert!(!marker.exists(), "excluded tool or fallback executed");
}
