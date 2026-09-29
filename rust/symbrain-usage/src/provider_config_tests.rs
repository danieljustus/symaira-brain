use super::{
    MAX_CREDENTIAL_FILE_BYTES, UsageFallbackSignals, claude_file_token_in, codex_file_token,
    codex_from_resolved, copilot_file_token_in, decode_base64url, is_secret_reference, json_string,
    kimi_store, names_from_keychain_dump, needs_go_fallback_for, nous_file_token, nous_jwt_is_live,
    read_limited,
};
use std::fs;
use std::path::{Path, PathBuf};

fn write(directory: &Path, name: &str, contents: &str) -> PathBuf {
    let path = directory.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent directory");
    }
    fs::write(&path, contents).expect("credential file");
    path
}

/// Test-only counterpart of `decode_base64url`, so the JWT cases can build the
/// payload they then read back.
fn encode_base64url(value: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut encoded = String::new();
    for chunk in value.as_bytes().chunks(3) {
        let mut buffer = u32::from(*chunk.first().expect("chunk"));
        buffer <<= 8;
        if let Some(second) = chunk.get(1) {
            buffer |= u32::from(*second);
        }
        buffer <<= 8;
        if let Some(third) = chunk.get(2) {
            buffer |= u32::from(*third);
        }
        for index in 0..=chunk.len() {
            let shift = 18 - index * 6;
            encoded.push(char::from(ALPHABET[(buffer >> shift & 0x3F) as usize]));
        }
    }
    encoded
}

fn jwt_expiring_at(seconds: u64) -> String {
    format!(
        "header.{}.signature",
        encode_base64url(&format!(r#"{{"exp":{seconds}}}"#))
    )
}

#[test]
fn bounded_credential_file_read_rejects_oversized_files() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("credential.json");
    let contents = vec![b'x'; usize::try_from(MAX_CREDENTIAL_FILE_BYTES).unwrap() + 1];
    fs::write(&path, contents).expect("credential file");
    assert!(read_limited(&path).is_none());
}

#[cfg(unix)]
#[test]
fn bounded_credential_file_read_does_not_follow_symlinks() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let target = directory.path().join("target.json");
    let link = directory.path().join("credential.json");
    fs::write(&target, b"{\"token\":\"value\"}").expect("credential target");
    std::os::unix::fs::symlink(&target, &link).expect("credential symlink");
    assert!(read_limited(&link).is_none());
}

#[test]
fn claude_token_prefers_the_default_account() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = write(
        directory.path(),
        ".credentials.json",
        r#"{"oauthAccount":{"work":{"accessToken":"work-token"},"default":{"accessToken":"default-token"}}}"#,
    );
    assert_eq!(
        claude_file_token_in(&path).as_deref(),
        Some("default-token")
    );
}

#[test]
fn claude_token_falls_back_to_any_account_with_a_token() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = write(
        directory.path(),
        ".credentials.json",
        r#"{"oauthAccount":{"work":{"accessToken":""},"spare":{"accessToken":"spare-token"}}}"#,
    );
    assert_eq!(claude_file_token_in(&path).as_deref(), Some("spare-token"));
}

#[test]
fn codex_token_reads_both_shipped_shapes() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        directory.path(),
        "auth.json",
        r#"{"access_token":"flat-token"}"#,
    );
    assert_eq!(
        codex_file_token(directory.path()).as_deref(),
        Some("flat-token")
    );
    write(
        directory.path(),
        "auth.json",
        r#"{"tokens":{"access_token":"nested-token"}}"#,
    );
    assert_eq!(
        codex_file_token(directory.path()).as_deref(),
        Some("nested-token")
    );
}

#[test]
fn codex_token_ignores_a_logged_out_auth_file() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        directory.path(),
        "auth.json",
        r#"{"OPENAI_API_KEY":"stale","tokens":{}}"#,
    );
    assert!(codex_file_token(directory.path()).is_none());
}

#[test]
fn codex_provider_reports_file_source_and_keeps_invalid_file_status() {
    let file = codex_from_resolved(
        Some(("file".into(), "synthetic-codex-file-fixture".into())),
        None,
        true,
    );
    assert!(file.configured);
    assert_eq!(file.auth_status.source.as_deref(), Some("file"));
    assert_eq!(file.auth_status.status, "available");
    assert_eq!(
        file.auth_status.detail,
        "Signed in via Codex CLI OAuth (CODEX_ACCESS_TOKEN or auth.json)"
    );

    let invalid = codex_from_resolved(None, None, true);
    assert!(!invalid.configured);
    assert_eq!(invalid.auth_status.status, "expired");
    assert_eq!(invalid.auth_status.source.as_deref(), Some("file"));
    assert_eq!(
        invalid.auth_status.detail,
        "Codex auth file found but no valid token — re-auth with the Codex CLI"
    );
}

#[test]
fn copilot_token_prefers_the_github_host_entry() {
    let directory = tempfile::tempdir().expect("temporary directory");
    // One matching host entry only: with several `github.com:` keys the shipped
    // implementation returns whichever its map iteration reaches first, so the
    // preference itself is not a contract.
    write(
        directory.path(),
        "apps.json",
        r#"{"github.com":{"oauth_token":"github-token"},"other.com":{"oauth_token":"other-token"}}"#,
    );
    assert_eq!(
        copilot_file_token_in(directory.path()).as_deref(),
        Some("github-token")
    );
}

#[test]
fn copilot_token_reads_hosts_json_after_apps_json() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        directory.path(),
        "apps.json",
        r#"{"other.com":{"oauth_token":""}}"#,
    );
    write(
        directory.path(),
        "hosts.json",
        r#"{"some.host":{"oauth_token":"hosts-token"}}"#,
    );
    assert_eq!(
        copilot_file_token_in(directory.path()).as_deref(),
        Some("hosts-token")
    );
}

#[test]
fn only_one_direct_provider_environment_credential_uses_native_reporting() {
    assert!(!needs_go_fallback_for(UsageFallbackSignals {
        claude_admin_env: Some("synthetic-claude-admin-env-fixture"),
        ..UsageFallbackSignals::default()
    }));
    assert!(!needs_go_fallback_for(UsageFallbackSignals {
        claude_oauth_env: Some("synthetic-claude-oauth-env-fixture"),
        ..UsageFallbackSignals::default()
    }));
    assert!(!needs_go_fallback_for(UsageFallbackSignals {
        copilot_env: Some("synthetic-copilot-fixture"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        copilot_env: Some("symvault://copilot/token"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        copilot_env: Some("synthetic-copilot-fixture"),
        other_provider_env: true,
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        copilot_env: Some("synthetic-copilot-fixture"),
        other_credential_source: true,
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        copilot_env: Some("synthetic-copilot-fixture"),
        local_provider_present: true,
        ..UsageFallbackSignals::default()
    }));
    assert!(!needs_go_fallback_for(UsageFallbackSignals {
        openrouter_env: Some("synthetic-openrouter-fixture"),
        ..UsageFallbackSignals::default()
    }));
    assert!(!needs_go_fallback_for(UsageFallbackSignals {
        moonshot_env: Some("synthetic-moonshot-fixture"),
        ..UsageFallbackSignals::default()
    }));
    assert!(!needs_go_fallback_for(UsageFallbackSignals {
        cursor_env: Some("synthetic-cursor-fixture"),
        ..UsageFallbackSignals::default()
    }));
    assert!(!needs_go_fallback_for(UsageFallbackSignals {
        kimi_api_env: Some("synthetic-kimi-api-fixture"),
        ..UsageFallbackSignals::default()
    }));
    assert!(!needs_go_fallback_for(UsageFallbackSignals {
        nous_env: Some("synthetic-nous-env-fixture"),
        ..UsageFallbackSignals::default()
    }));
    assert!(!needs_go_fallback_for(UsageFallbackSignals {
        codex_env: Some("synthetic-codex-env-fixture"),
        ..UsageFallbackSignals::default()
    }));
    assert!(!needs_go_fallback_for(UsageFallbackSignals {
        opencode_env: Some("synthetic-opencode-cookie"),
        ..UsageFallbackSignals::default()
    }));
    assert!(!needs_go_fallback_for(UsageFallbackSignals {
        opencode_workspace_override: Some("wrk_workspace"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        opencode_env: Some("synthetic-opencode-cookie"),
        opencode_workspace_override: Some("wrk_workspace"),
        ..UsageFallbackSignals::default()
    }));
}

#[test]
fn claude_file_credential_uses_native_reporting_only_when_it_is_the_sole_source() {
    assert!(!needs_go_fallback_for(UsageFallbackSignals {
        claude_file: Some("synthetic-claude-file-fixture"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        claude_file: Some("synthetic-claude-file-fixture"),
        claude_oauth_env: Some("synthetic-claude-oauth-env-fixture"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        claude_file: Some("synthetic-claude-file-fixture"),
        claude_admin_env: Some("synthetic-claude-admin-env-fixture"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        claude_file: Some("synthetic-claude-file-fixture"),
        other_provider_env: true,
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        claude_file: Some("synthetic-claude-file-fixture"),
        other_credential_source: true,
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        claude_file: Some("synthetic-claude-file-fixture"),
        local_provider_present: true,
        ..UsageFallbackSignals::default()
    }));
    for reference in [
        "symvault://claude/oauth-token",
        "vault://claude/oauth-token",
        "env://ANTHROPIC_OAUTH_TOKEN",
        "keychain://Claude Code-credentials/account",
    ] {
        assert!(needs_go_fallback_for(UsageFallbackSignals {
            claude_file: Some(reference),
            ..UsageFallbackSignals::default()
        }));
    }
}

#[test]
fn codex_file_credential_uses_native_reporting_only_when_it_is_the_sole_source() {
    assert!(!needs_go_fallback_for(UsageFallbackSignals {
        codex_file: Some("synthetic-codex-file-fixture"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        codex_file: Some("synthetic-codex-file-fixture"),
        codex_env: Some("synthetic-codex-env-fixture"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        codex_file: Some("synthetic-codex-file-fixture"),
        other_provider_env: true,
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        codex_file: Some("synthetic-codex-file-fixture"),
        other_credential_source: true,
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        codex_file: Some("synthetic-codex-file-fixture"),
        local_provider_present: true,
        ..UsageFallbackSignals::default()
    }));
    for reference in [
        "symvault://codex/access-token",
        "vault://codex/access-token",
        "env://CODEX_ACCESS_TOKEN",
        "keychain://Codex/account",
    ] {
        assert!(is_secret_reference(reference));
        assert!(needs_go_fallback_for(UsageFallbackSignals {
            codex_file: Some(reference),
            ..UsageFallbackSignals::default()
        }));
    }
}

#[test]
fn secret_references_overrides_and_multiple_credentials_keep_go_fallback() {
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        claude_admin_env: Some("symvault://claude/admin-key"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        claude_oauth_env: Some("symvault://claude/oauth-token"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        claude_admin_env: Some("synthetic-claude-admin-env-fixture"),
        claude_oauth_env: Some("synthetic-claude-oauth-env-fixture"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        claude_oauth_env: Some("synthetic-claude-oauth-env-fixture"),
        other_provider_env: true,
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        claude_oauth_env: Some("synthetic-claude-oauth-env-fixture"),
        other_credential_source: true,
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        claude_oauth_env: Some("synthetic-claude-oauth-env-fixture"),
        local_provider_present: true,
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        claude_admin_env: Some("synthetic-claude-admin-env-fixture"),
        other_provider_env: true,
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        openrouter_env: Some("symvault://openrouter/key"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        cursor_env: Some("symvault://cursor/cookie"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        openrouter_env: Some("synthetic-openrouter-fixture"),
        other_provider_env: true,
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        copilot_env: Some("synthetic-copilot-fixture"),
        openrouter_env: Some("synthetic-openrouter-fixture"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        moonshot_env: Some("symvault://moonshot/key"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        moonshot_env: Some("synthetic-moonshot-fixture"),
        // A non-empty MOONSHOT_REGION, including explicit `ai`, remains Go.
        other_provider_env: true,
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        openrouter_env: Some("synthetic-openrouter-fixture"),
        moonshot_env: Some("synthetic-moonshot-fixture"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        cursor_env: Some("synthetic-cursor-fixture"),
        other_provider_env: true,
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        kimi_api_env: Some("symvault://kimi/api-key"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        kimi_api_env: Some("synthetic-kimi-api-fixture"),
        other_provider_env: true,
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        nous_env: Some("symvault://nous/access-token"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        nous_env: Some("synthetic-nous-env-fixture"),
        other_provider_env: true,
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        codex_env: Some("symvault://codex/access-token"),
        ..UsageFallbackSignals::default()
    }));
    assert!(needs_go_fallback_for(UsageFallbackSignals {
        codex_env: Some("synthetic-codex-env-fixture"),
        other_provider_env: true,
        ..UsageFallbackSignals::default()
    }));
}

#[test]
fn kimi_store_reads_token_and_trims_the_device_id() {
    let directory = tempfile::tempdir().expect("temporary directory");
    write(
        directory.path(),
        "credentials/kimi-code.json",
        r#"{"access_token":"kimi-token"}"#,
    );
    write(directory.path(), "device_id", "device-42\n");
    assert_eq!(
        kimi_store(directory.path()),
        (Some("kimi-token".to_owned()), Some("device-42".to_owned()))
    );
}

#[test]
fn nous_token_prefers_the_scoped_invoke_jwt() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = write(
        directory.path(),
        "auth.json",
        r#"{"providers":[{"id":"other","access_token":"other-token"},{"id":"nous","invoke_jwt":"plain-jwt","access_token":"nous-token"}]}"#,
    );
    assert_eq!(nous_file_token(&path).as_deref(), Some("plain-jwt"));
}

#[test]
fn nous_token_ignores_providers_other_than_nous() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = write(
        directory.path(),
        "auth.json",
        r#"{"providers":[{"id":"anthropic","access_token":"anthropic-token"}]}"#,
    );
    assert!(nous_file_token(&path).is_none());
}

#[test]
fn nous_token_skips_jwt_shaped_tokens_that_expired() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = write(
        directory.path(),
        "auth.json",
        &format!(
            r#"{{"providers":[{{"id":"nous","invoke_jwt":"{}"}}]}}"#,
            jwt_expiring_at(1)
        ),
    );
    // An expired JWT counts as "not signed in" rather than yielding a token the
    // endpoint would reject.
    assert!(nous_file_token(&path).is_none());
}

#[test]
fn nous_token_accepts_a_live_jwt() {
    let future = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .expect("clock")
        .as_secs()
        + 3_600;
    assert!(nous_jwt_is_live(&jwt_expiring_at(future)));
    assert!(!nous_jwt_is_live("not-a-jwt"));
    assert!(!nous_jwt_is_live("only.two"));
    assert!(!nous_jwt_is_live("no.exp-claim.signature"));
}

#[test]
fn keychain_dump_lines_yield_service_names() {
    let dump = concat!(
        "keychain: \"/Users/dev/Library/Keychains/login.keychain-db\"\n",
        "    \"svce\"<blob>=\"Claude Code-credentials\"\n",
        "    \"svce\"<blob>=\"Claude Code-credentials-552ffa86\"\n",
        "    \"acct\"<blob>=\"dev\"\n",
        "class: 0x00000000 \n",
    );
    assert_eq!(
        names_from_keychain_dump(dump),
        vec![
            "Claude Code-credentials".to_owned(),
            "Claude Code-credentials-552ffa86".to_owned()
        ]
    );
}

#[test]
fn base64url_decoding_round_trips_a_json_payload() {
    assert_eq!(
        decode_base64url("eyJleHAiOjF9").as_deref(),
        Some(&b"{\"exp\":1}"[..])
    );
    assert!(decode_base64url("not*base64").is_none());
}

#[test]
fn json_pointer_lookup_ignores_missing_and_empty_values() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = write(directory.path(), "auth.json", r#"{"access_token":""}"#);
    assert!(json_string(&path, &["access_token"]).is_none());
    assert!(json_string(&path, &["tokens", "access_token"]).is_none());
}
