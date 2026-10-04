//! Exercise the shared Brain/standalone decision boundary, not a formatter copy.
#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
// Compile the exact Brain compatibility adapter against the shared implementation.
#[path = "../../symbrain-cli/src/guard_cli.rs"]
mod brain_adapter;
use brain_adapter::run_at_path;
#[path = "support/raw_fixture_admission.rs"]
mod raw_fixture_admission;

#[test]
fn raw_audit_path_denies_allow_and_confirm_without_overwriting_existing_denials() {
    let root = tempfile::tempdir().unwrap();
    let mut help = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        brain_adapter::run(&["help".into()], &mut help, &mut stderr),
        Some(0)
    );
    assert!(!help.is_empty() && stderr.is_empty());
    let path = root.path().join(OsString::from_vec(
        raw_fixture_admission::COMPONENT.to_vec(),
    ));
    let before = raw_fixture_admission::entries(root.path());
    if let Err(error) = std::fs::create_dir(&path) {
        raw_fixture_admission::record_failed_creation(
            root,
            &path,
            raw_fixture_admission::Operation::AuditDirectory,
            error,
            before,
            raw_fixture_admission::Coverage {
                requested: &[
                    "help",
                    "low",
                    "medium",
                    "high",
                    "critical",
                    "unknown",
                    "invalid-json",
                ],
                executed: &["help"],
                unavailable: &[
                    "low",
                    "medium",
                    "high",
                    "critical",
                    "unknown",
                    "invalid-json",
                ],
            },
        );
        return;
    }
    let now = "2026-09-14T12:00:00Z".parse().unwrap();
    for risk in ["low", "medium", "high", "critical", "unknown"] {
        let payload = format!(r#"{{"command":"owned","risk_class":"{risk}"}}"#);
        let mut out = Vec::new();
        assert_eq!(
            run_at_path(payload.as_bytes(), &mut out, path.clone(), now),
            0
        );
        let reply: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(reply["decision"], "deny");
        if matches!(risk, "low" | "medium" | "high") {
            assert!(out.windows(12).any(|bytes| bytes == br"\ufffd\ufffd"));
            assert!(out.windows(18).any(|bytes| bytes == br"\u003c\u0026\u003e"));
            assert!(
                reply["reason"]
                    .as_str()
                    .unwrap()
                    .starts_with("audit: write decision record:")
            );
        } else {
            assert!(!reply["reason"].as_str().unwrap().contains("audit: "));
        }
    }
    let mut out = Vec::new();
    assert_eq!(run_at_path(b"not json".as_slice(), &mut out, path, now), 0);
    let reply: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(
        reply["reason"]
            .as_str()
            .unwrap()
            .starts_with("decide: parse request:")
    );
}
