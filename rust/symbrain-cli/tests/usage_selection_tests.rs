//! CLI flag validation and provider selection are credential-free and native.
#[path = "../../test-support/coverage.rs"]
mod coverage;
#[cfg(unix)]
#[test]
fn usage_provider_selection_is_native_and_credential_free() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let home = tempfile::tempdir().unwrap();
    let marker = home.path().join("accessed");
    for name in ["symvault", "security", "ps", "lsof", "gh"] {
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
    let fallback = home.path().join("go-fallback");
    std::fs::write(&fallback, "#!/bin/sh\nprintf '%s\\n' \"$@\"\nexit 0\n").unwrap();
    std::fs::set_permissions(&fallback, std::fs::Permissions::from_mode(0o700)).unwrap();
    let run = |args: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
        command
            .args(args)
            .env_clear()
            .envs(coverage::profile_environment())
            .env("HOME", home.path())
            .env("CODEX_HOME", home.path())
            .env("PATH", home.path())
            .env("SYMBRAIN_GO_BINARY", &fallback)
            .env("KIMI_CODE_BASE_URL", "http://unsupported.invalid");
        for key in [
            "ANTHROPIC_ADMIN_KEY",
            "COPILOT_ACCESS_TOKEN",
            "CURSOR_COOKIE",
            "KIMI_CODE_API_KEY",
            "KIMI_AUTH_TOKEN",
            "MOONSHOT_API_KEY",
            "NOUS_PORTAL_ACCESS_TOKEN",
            "OPENCODE_COOKIE",
            "ZAI_API_KEY",
            "DEVIN_API_KEY",
            "GROK_ACCESS_TOKEN",
        ] {
            command.env(key, "symvault://test/excluded");
        }
        command.output().unwrap()
    };
    for args in [
        vec![
            "usage",
            "--output",
            "json",
            "--providers",
            "openrouter,codex,codex",
        ],
        vec!["usage", "--providers=codex,openrouter", "--json"],
    ] {
        let result = run(&args);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        let ids: Vec<_> = report["providers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["codex", "openrouter"]);
    }
    for selector in ["", "unknown", "codex,unknown", "codex,", ",codex"] {
        let result = run(&["usage", "--output", "json", "--providers", selector]);
        assert_eq!(result.status.code(), Some(2));
        assert!(result.stdout.is_empty());
    }
    // An unusual base override is no longer a routing predicate: the same
    // invocation is answered natively and never reaches the fallback script.
    let result = run(&["usage", "--output", "json", "--providers", "openrouter"]);
    assert!(result.status.success());
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    let providers = report["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0]["id"], "openrouter");
    assert_eq!(providers[0]["configured"], false);
    assert!(
        !marker.exists(),
        "excluded credential/process command executed"
    );
}
