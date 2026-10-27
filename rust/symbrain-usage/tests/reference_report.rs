//! Issue #930: isolated subprocess fixtures, never the operator's vault or network.
#![cfg(unix)]
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt, process::Command, sync::Arc};
use symbrain_usage::{FixtureTransport, Response, Service, all_providers};

const CREDENTIALS: &[&str] = &[
    "ANTHROPIC_ADMIN_KEY",
    "ANTHROPIC_OAUTH_TOKEN",
    "CODEX_ACCESS_TOKEN",
    "COPILOT_ACCESS_TOKEN",
    "CURSOR_COOKIE",
    "KIMI_CODE_API_KEY",
    "KIMI_AUTH_TOKEN",
    "MOONSHOT_API_KEY",
    "NOUS_PORTAL_ACCESS_TOKEN",
    "OPENCODE_COOKIE",
    "OPENROUTER_API_KEY",
];

#[test]
fn reference_report() {
    for scheme in ["symvault://", "vault://"] {
        for mode in ["ok", "missing", "locked", "store", "binary"] {
            let dir = tempfile::tempdir().expect("fixture directory");
            if mode != "binary" {
                let script = format!(
                    "#!/bin/sh\n[ \"$1\" = get ] && [ \"$2\" = -- ] && [ \"$3\" = fixture/openrouter ] && [ \"$4\" = --print ] || exit 9\nprintf 'fixture-secret\\n'\n{}",
                    if mode == "ok" {
                        String::new()
                    } else {
                        format!("printf '{mode}\\n' >&2; exit 1\n")
                    }
                );
                let binary = dir.path().join("symvault");
                fs::write(&binary, script).expect("fake vault");
                fs::set_permissions(binary, fs::Permissions::from_mode(0o700)).expect("executable");
            }
            let mut child = Command::new(std::env::current_exe().expect("test executable"));
            child
                .args(["--exact", "reference_report_child", "--nocapture"])
                .env_clear()
                .env("PATH", dir.path())
                .env("HOME", dir.path())
                .env("REFERENCE_MODE", mode);
            for name in CREDENTIALS {
                child.env(name, format!("{scheme}fixture/openrouter"));
            }
            let result = child.output().expect("fixture child");
            assert!(
                result.status.success(),
                "{scheme} {mode}: {}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
}

#[test]
fn reference_report_child() {
    let Ok(mode) = std::env::var("REFERENCE_MODE") else {
        return;
    };
    let providers = all_providers();
    let transport = Arc::new(FixtureTransport::new(BTreeMap::from([(
        "openrouter".into(),
        Response {
            status: 200,
            body: br#"{"data":{"usage":1}}"#.to_vec(),
            headers: BTreeMap::new(),
        },
    )])));
    if mode == "ok" {
        let provider = providers
            .into_iter()
            .find(|p| p.id == "openrouter")
            .expect("provider");
        let report = Service::with_transport(vec![provider], transport.clone()).report();
        assert!(report.providers[0].configured);
        assert!(report.providers[0].snapshot.is_some());
        let requests = transport.requests();
        assert_eq!(requests.len(), 1);
        assert!(
            requests[0]
                .headers
                .values()
                .any(|v| v == "Bearer fixture-secret")
        );
    } else {
        let report = Service::with_transport(
            providers
                .into_iter()
                .filter(|p| p.id != "antigravity")
                .collect(),
            transport.clone(),
        )
        .report();
        assert_eq!(report.providers.len(), 9);
        for provider in report.providers {
            assert!(provider.configured, "{} must be configured", provider.id);
            let error = provider.error.expect("reference resolution error");
            assert!(error.contains("credential resolution failed:"));
            assert!(error.contains("symvault://fixture/openrouter"));
            assert!(!error.contains("fixture-secret"));
        }
        assert!(transport.requests().is_empty());
    }
}
