//! Fresh-process public API tests; only synthetic credentials and fixture transport.
#[path = "../../test-support/coverage.rs"]
mod coverage;
use std::process::Command;
use symbrain_usage::{ProviderSelection, selected_providers};

fn check_child(case: &str) {
    let selection = ProviderSelection::parse("opencode").unwrap();
    assert!(!symbrain_usage::needs_go_fallback_for_selection(&selection));
    let providers = selected_providers(&selection);
    let p = &providers[0];
    if ["v2", "failed", "invalid"].contains(&case) {
        assert!(!p.configured);
        assert!(!format!("{:?}", p.auth_status).contains("synthetic-secret"));
        return;
    }
    assert!(p.configured);
    let fixture = std::sync::Arc::new(symbrain_usage::FixtureTransport::new(
        [(
            "opencode".into(),
            symbrain_usage::Response {
                status: 200,
                headers: std::collections::BTreeMap::new(),
                body: br#"{"usage":{"weekly":{"percent":0}}}"#.to_vec(),
            },
        )]
        .into(),
    ));
    let report = symbrain_usage::Service::with_transport(providers, fixture.clone()).report();
    let requests = fixture.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url, "https://opencode.ai/zen/go/v1/usage");
    assert_eq!(
        requests[0].headers.get("Authorization").unwrap(),
        "Bearer synthetic-secret"
    );
    assert!(!requests[0].headers.contains_key("Cookie"));
    assert_eq!(
        report.providers[0].snapshot.as_ref().unwrap().meters[0]
            .used
            .as_deref(),
        Some("0")
    );
}

#[test]
fn opencode_api_credentials() {
    if let Ok(case) = std::env::var("OPENCODE_API_CHILD") {
        check_child(&case);
        return;
    }
    for case in [
        "env", "vault", "file", "xdg", "default", "v2", "failed", "invalid",
    ] {
        let root = tempfile::tempdir().unwrap();
        let data = match case {
            "xdg" => root.path().join("data/opencode"),
            "default" => root.path().join(".local/share/opencode"),
            _ => root.path().join("opencode"),
        };
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(
            data.join("auth.json"),
            br#"{"opencode-go":{"key":"synthetic-secret"}}"#,
        )
        .unwrap();
        if case == "v2" {
            std::fs::write(data.join("opencode.db"), []).unwrap();
        }
        let bin = root.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let fake = bin.join("symvault");
            std::fs::write(&fake, "#!/bin/sh\nprintf 'synthetic-secret\\n'\n").unwrap();
            std::fs::set_permissions(fake, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "opencode_api_credentials", "--nocapture"])
            .env_clear()
            .envs(coverage::profile_environment())
            .env("HOME", root.path())
            .env("USERPROFILE", root.path())
            .env("PATH", &bin)
            .env("OPENCODE_API_CHILD", case);
        if case == "xdg" {
            cmd.env("XDG_DATA_HOME", root.path().join("data"));
        } else if case != "default" {
            cmd.env("OPENCODE_DATA_DIR", &data);
        }
        match case {
            "env" => {
                cmd.env("OPENCODE_GO_API_KEY", "env://OPENCODE_SYNTHETIC_KEY")
                    .env("OPENCODE_SYNTHETIC_KEY", "synthetic-secret")
                    .env("OPENCODE_COOKIE", "unrelated")
                    .env("OPENCODE_WORKSPACE_ID", "unsupported spelling");
            }
            "vault" => {
                #[cfg(not(unix))]
                {
                    continue;
                }
                cmd.env("OPENCODE_GO_API_KEY", "symvault://test/opencode");
            }
            "failed" => {
                cmd.env("OPENCODE_GO_API_KEY", "env://MISSING_SYNTHETIC_KEY")
                    .env("OPENCODE_COOKIE", "unrelated");
            }
            "invalid" => {
                cmd.env("OPENCODE_GO_API_KEY", "\n")
                    .env("OPENCODE_COOKIE", "unrelated");
            }
            _ => {}
        }
        let output = cmd.output().unwrap();
        assert!(
            output.status.success(),
            "{case}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
