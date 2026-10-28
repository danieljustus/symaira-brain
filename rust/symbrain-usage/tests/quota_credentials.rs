//! Fresh-process credential precedence tests: no inherited environment or real tools.
use std::process::Command;
use symbrain_usage::{Provider, ProviderSelection, selected_providers};

fn check_child(case: &str) {
    let providers = selected_providers(&ProviderSelection::parse("zai,devin").unwrap());
    let zai = providers.iter().find(|p| p.id == "zai").unwrap();
    let devin = providers.iter().find(|p| p.id == "devin").unwrap();
    match case {
        "missing" => {
            assert!(!zai.configured);
            assert!(!devin.configured);
        }
        "env" | "vault" => {
            assert!(zai.configured && devin.configured);
            let expected = if case == "vault" { "vault" } else { "env" };
            assert_eq!(zai.auth_status.source.as_deref(), Some(expected));
            assert_eq!(devin.auth_status.source.as_deref(), Some(expected));
            for p in [zai, devin] {
                assert!(!format!("{p:?}").contains("hermetic-secret"));
            }
        }
        "file" => {
            assert!(!zai.configured);
            assert!(devin.configured);
            assert_eq!(devin.auth_status.source.as_deref(), Some("file"));
        }
        "failed" => {
            assert!(!devin.configured);
            assert_eq!(devin.auth_status.detail, "credential resolution failed");
        }
        _ => panic!("unknown case"),
    }
    assert!(!symbrain_usage::needs_go_fallback());
    if ["env", "vault", "file"].contains(&case) {
        check_report(providers, case);
    }
}
fn check_report(providers: Vec<Provider>, case: &str) {
    let quota = providers
        .into_iter()
        .filter(|p| ["zai", "devin"].contains(&p.id.as_str()))
        .collect();
    let responses = [
        (
            "zai".into(),
            symbrain_usage::Response {
                status: 200,
                headers: std::collections::BTreeMap::new(),
                body: br#"{"limits":[]}"#.to_vec(),
            },
        ),
        (
            "devin".into(),
            symbrain_usage::Response {
                status: 200,
                headers: std::collections::BTreeMap::new(),
                body: br#"{"userStatus":{"planStatus":{"weeklyQuotaRemainingPercent":0}}}"#
                    .to_vec(),
            },
        ),
    ]
    .into();
    let transport = std::sync::Arc::new(symbrain_usage::FixtureTransport::new(responses));
    let report = symbrain_usage::Service::with_transport(quota, transport.clone()).report();
    for provider in report.providers.iter().filter(|p| p.configured) {
        assert!(provider.snapshot.is_some(), "{provider:?}");
    }
    for request in transport.requests() {
        let expected = if case == "file" {
            "file-secret"
        } else {
            "hermetic-secret"
        };
        if request.provider_id == "zai" {
            assert_eq!(
                request.headers.get("Authorization").unwrap(),
                &format!("Bearer {expected}")
            );
        } else {
            let body: serde_json::Value =
                serde_json::from_slice(request.body.as_ref().unwrap()).unwrap();
            assert_eq!(body["metadata"]["apiKey"], expected);
            assert!(request.url.starts_with("https://server.codeium.com/"));
        }
    }
}
#[test]
fn quota_credentials() {
    if let Ok(case) = std::env::var("QUOTA_CHILD_CASE") {
        check_child(&case);
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let path = home.join(".local/share/devin/credentials.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bin = root.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let fake = bin.join("symvault");
        std::fs::write(&fake, "#!/bin/sh\nprintf 'hermetic-secret\\n'\n").unwrap();
        std::fs::set_permissions(fake, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    for case in ["missing", "env", "vault", "file", "failed"] {
        if case != "missing" {
            std::fs::write(
                &path,
                "windsurf_api_key = 'file-secret'\napi_server_url = 'https://untrusted.example'\n",
            )
            .unwrap();
        }
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "quota_credentials", "--nocapture"])
            .env_clear()
            .env("QUOTA_CHILD_CASE", case)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("PATH", &bin)
            // Explicit dummy OAuth prevents even a system Keychain listing.
            .env("ANTHROPIC_OAUTH_TOKEN", "hermetic-claude");
        if case == "env" {
            cmd.env("ZAI_API_KEY", "env://QUOTA_TOKEN")
                .env("DEVIN_API_KEY", "env://QUOTA_TOKEN")
                .env("QUOTA_TOKEN", "hermetic-secret");
        }
        if case == "vault" {
            #[cfg(not(unix))]
            {
                continue;
            }
            cmd.env("ZAI_API_KEY", "symvault://test/zai")
                .env("DEVIN_API_KEY", "symvault://test/devin");
        }
        if case == "failed" {
            cmd.env("DEVIN_API_KEY", "env://QUOTA_NOT_SET");
        }
        let result = cmd.output().unwrap();
        assert!(
            result.status.success(),
            "{case}: {} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
