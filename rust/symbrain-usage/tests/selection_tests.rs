//! Selection is tested in a clean subprocess so no real credentials are visible.
#[cfg(unix)]
#[test]
fn provider_selection_excludes_credential_discovery_and_routing() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use symbrain_usage::{ProviderSelection, Service, needs_go_fallback_for_selection};

    if std::env::var_os("SELECTION_TEST_CHILD").is_some() {
        for selector in [
            "",
            ",",
            "codex,",
            ",codex",
            "codex,unknown",
            "Codex",
            " codex",
        ] {
            assert!(ProviderSelection::parse(selector).is_err(), "{selector:?}");
        }
        let selection = ProviderSelection::parse("openrouter,codex,codex").unwrap();
        assert!(!needs_go_fallback_for_selection(&selection));
        let service = Service::with_selection(&selection);
        let report = service.report();
        assert_eq!(
            report
                .providers
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            ["codex", "openrouter"]
        );
        assert!(report.providers.iter().all(|p| !p.configured));
        assert!(!std::path::Path::new(&std::env::var_os("SELECTION_MARKER").unwrap()).exists());
        // Positive route control: an explicitly selected unsupported source still falls back.
        assert!(needs_go_fallback_for_selection(
            &ProviderSelection::parse("kimi").unwrap()
        ));
        return;
    }

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
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args([
            "--exact",
            "provider_selection_excludes_credential_discovery_and_routing",
            "--nocapture",
        ])
        .env_clear()
        .env("HOME", home.path())
        .env("CODEX_HOME", home.path())
        .env("PATH", home.path())
        .env("SELECTION_TEST_CHILD", "1")
        .env("SELECTION_MARKER", &marker)
        .env("KIMI_CODE_BASE_URL", "http://unsupported.invalid")
        .env("HERMES_PORTAL_BASE_URL", "http://unsupported.invalid")
        .env("OPENCODE_WORKSPACE_ID", "unsupported workspace");
    for key in [
        "ANTHROPIC_ADMIN_KEY",
        "ANTHROPIC_OAUTH_TOKEN",
        "COPILOT_ACCESS_TOKEN",
        "CURSOR_COOKIE",
        "KIMI_CODE_API_KEY",
        "KIMI_AUTH_TOKEN",
        "MOONSHOT_API_KEY",
        "NOUS_PORTAL_ACCESS_TOKEN",
        "OPENCODE_COOKIE",
    ] {
        child.env(key, "symvault://test/excluded");
    }
    let result = child.output().unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!marker.exists(), "excluded command executed");
}
