use super::{AccountRef, ClaudeScan, CodexScan, claude_accounts, codex_accounts, label};

const UUID_A: &str = "11111111-aaaa-4aaa-8aaa-111111111111";
const UUID_B: &str = "22222222-bbbb-4bbb-8bbb-222222222222";

fn states(accounts: &[AccountRef]) -> Vec<(String, String)> {
    accounts
        .iter()
        .map(|a| (a.label.clone(), a.state.clone()))
        .collect()
}

fn pair(label: &str, state: &str) -> (String, String) {
    (label.to_owned(), state.to_owned())
}

fn keychain_scan() -> ClaudeScan {
    ClaudeScan {
        oauth_source: Some("keychain".into()),
        keychain_service: Some("Claude Code-credentials".into()),
        cli_account_id: Some(UUID_A.into()),
        ..ClaudeScan::default()
    }
}

#[test]
fn labels_are_stable_non_secret_and_case_insensitive() {
    assert_eq!(
        label("claude", UUID_A),
        label("claude", &UUID_A.to_uppercase())
    );
    assert_ne!(label("claude", UUID_A), label("claude", UUID_B));
    let l = label("claude", UUID_A);
    assert!(
        l.starts_with("claude-") && l.len() == "claude-".len() + 8,
        "{l}"
    );
    assert!(!l.contains(&UUID_A[..8]));
}

#[test]
fn zero_accounts_emit_nothing() {
    assert!(claude_accounts(&ClaudeScan::default()).is_empty());
    assert!(codex_accounts(&CodexScan::default()).is_empty());
    // Desktop login alone, no credential in use: still nothing to attribute.
    let desktop_only = ClaudeScan {
        desktop_account_id: Some(UUID_B.into()),
        ..ClaudeScan::default()
    };
    assert!(claude_accounts(&desktop_only).is_empty());
}

#[test]
fn one_account_keeps_the_frozen_report_shape() {
    assert!(claude_accounts(&keychain_scan()).is_empty());
    let file = ClaudeScan {
        oauth_source: Some("file".into()),
        cli_account_id: Some(UUID_A.into()),
        ..ClaudeScan::default()
    };
    assert!(claude_accounts(&file).is_empty());
    let codex = CodexScan {
        token_source: Some("file".into()),
        file_account_id: Some(UUID_A.into()),
    };
    assert!(codex_accounts(&codex).is_empty());
}

#[test]
fn many_claude_logins_name_the_account_in_use_and_never_merge() {
    let mut scan = keychain_scan();
    scan.keychain_services = vec![
        "Claude Code-credentials".into(),
        "Claude Code-credentials-552ffa86".into(),
    ];
    scan.desktop_account_id = Some(UUID_B.into());
    let accounts = claude_accounts(&scan);
    assert_eq!(
        states(&accounts),
        [
            pair(&label("claude", UUID_A), "used"),
            pair("claude-keychain-552ffa86", "not_read"),
            pair(&label("claude", UUID_B), "not_read"),
        ]
    );
    // Exactly one account feeds the row.
    assert_eq!(accounts.iter().filter(|a| a.state == "used").count(), 1);

    // The suffixed item is the one the reader took: it becomes the used one.
    scan.keychain_service = Some("Claude Code-credentials-552ffa86".into());
    let accounts = claude_accounts(&scan);
    let used = accounts.iter().find(|a| a.state == "used").unwrap();
    assert_eq!(used.label, "claude-keychain-552ffa86");
    assert_eq!(accounts.iter().filter(|a| a.state == "used").count(), 1);
}

#[test]
fn desktop_login_equal_to_the_cli_login_is_one_account() {
    let mut scan = keychain_scan();
    scan.desktop_account_id = Some(UUID_A.into());
    assert!(claude_accounts(&scan).is_empty());
}

#[test]
fn ambiguous_claude_credentials_are_unattributed() {
    for (scan, why) in [
        (
            ClaudeScan {
                oauth_source: Some("env".into()),
                cli_account_id: Some(UUID_A.into()),
                ..ClaudeScan::default()
            },
            "environment",
        ),
        (
            ClaudeScan {
                admin_key: true,
                oauth_source: Some("file".into()),
                cli_account_id: Some(UUID_A.into()),
                ..ClaudeScan::default()
            },
            "Admin API key",
        ),
        (
            ClaudeScan {
                custom_config_dir: true,
                ..keychain_scan()
            },
            "CLAUDE_CONFIG_DIR",
        ),
    ] {
        let accounts = claude_accounts(&scan);
        assert_eq!(accounts[0].state, "unattributed");
        assert_eq!(accounts[0].label, "unattributed");
        assert!(accounts[0].detail.as_deref().unwrap().contains(why));
        assert_eq!(accounts[1].state, "candidate");
        assert!(!accounts.iter().any(|a| a.state == "used"));
    }
}

#[test]
fn codex_env_token_beside_a_file_login_is_unattributed() {
    let scan = CodexScan {
        token_source: Some("env".into()),
        file_account_id: Some(UUID_A.into()),
    };
    let accounts = codex_accounts(&scan);
    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[0].state, "unattributed");
    assert_eq!(accounts[1].label, label("codex", UUID_A));
    assert_eq!(accounts[1].state, "candidate");
    assert_eq!(accounts[1].source, "codex_auth_file");
    // No file login to compare with: nothing to disambiguate.
    let lone = CodexScan {
        token_source: Some("env".into()),
        file_account_id: None,
    };
    assert!(codex_accounts(&lone).is_empty());
}
