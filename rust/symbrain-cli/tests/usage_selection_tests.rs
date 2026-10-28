//! CLI flag validation is credential-free, including the whole-report Go route.
#[cfg(unix)]
#[test]
fn usage_provider_selection_precedes_fallback_and_survives_delegation() {
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
    let args = ["usage", "--output", "json", "--providers", "kimi"];
    let result = run(&args);
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        args.join("\n") + "\n",
        "fallback must retain original selection"
    );
    assert!(
        !marker.exists(),
        "excluded credential/process command executed"
    );
}
