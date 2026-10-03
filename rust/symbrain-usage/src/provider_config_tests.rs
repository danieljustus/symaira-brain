use super::{
    MAX_CREDENTIAL_FILE_BYTES, UsageFallbackSignals, claude_file_token_in, codex_file_token,
    codex_from_resolved, copilot_file_token_candidate_in, copilot_file_token_in, decode_base64url,
    json_string, kimi_device_id_is_native, kimi_file_token_candidate, kimi_store,
    names_from_keychain_dump, needs_go_fallback_for, nous_file_token, nous_file_token_candidate,
    nous_jwt_is_live, parse_claude_keychain_blob, path_may_exist, read_limited,
    supported_custom_base, supported_opencode_workspace,
};
use std::fs;
use std::path::{Path, PathBuf};

const CLAUDE_FILE_TOKEN_ORACLE: &str =
    include_str!("../tests/fixtures/claude_file_token_oracle.json");
const COPILOT_FILE_TOKEN_ORACLE: &str =
    include_str!("../tests/fixtures/copilot_file_token_oracle.json");
const NOUS_FILE_TOKEN_ORACLE: &str = include_str!("../tests/fixtures/nous_file_token_oracle.json");
const KIMI_FILE_TOKEN_ORACLE: &str = include_str!("../tests/fixtures/kimi_file_token_oracle.json");
const CLAUDE_KEYCHAIN_TOKEN_ORACLE: &str =
    include_str!("../tests/fixtures/claude_keychain_token_oracle.json");

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

#[test]
fn routing_metadata_treats_indeterminate_and_symlink_paths_as_present() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let missing = directory.path().join("missing/auth.json");
    assert!(!path_may_exist(&missing));

    let ordinary = write(directory.path(), "ordinary/auth.json", "{}");
    assert!(path_may_exist(&ordinary));

    // A metadata error other than NotFound is conservatively treated as a
    // possible credential path; this Unix case is a non-directory parent.
    #[cfg(unix)]
    let not_directory = write(directory.path(), "file-parent", "x").join("auth.json");
    #[cfg(unix)]
    assert!(path_may_exist(&not_directory));

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let dangling = directory.path().join("dangling/auth.json");
        fs::create_dir_all(dangling.parent().expect("parent")).expect("create parent");
        symlink(directory.path().join("absent-target"), &dangling).expect("create symlink");
        assert!(path_may_exist(&dangling));
    }
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
fn claude_file_token_accepts_only_go_equivalent_deterministic_shapes() {
    let fixture: serde_json::Value =
        serde_json::from_str(CLAUDE_FILE_TOKEN_ORACLE).expect("Go Claude parser oracle");
    for case in fixture["cases"].as_array().expect("oracle cases") {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = write(
            directory.path(),
            ".credentials.json",
            case["contents"].as_str().expect("file contents"),
        );
        let got = claude_file_token_in(&path);
        let id = case["id"].as_str().expect("case id");
        match id {
            "default-account-precedes-other-accounts"
            | "single-nondefault-account-is-unambiguous"
            | "duplicate-account-key-uses-last-token" => {
                assert_eq!(
                    got.as_deref(),
                    case["token"].as_str(),
                    "safe candidate must match Go case {id}"
                );
            }
            _ => {
                assert!(
                    case["token"].is_string() || case["possible_tokens"].is_array(),
                    "Go oracle case {id} must record an exact token or its normalized choices"
                );
                assert!(
                    got.is_none(),
                    "unproven Go case {id} must remain on the Go path, got {got:?}"
                );
            }
        }
    }
}

#[test]
fn copilot_file_token_matches_go_for_native_shapes_and_rejects_unproven_files() {
    let fixture: serde_json::Value =
        serde_json::from_str(COPILOT_FILE_TOKEN_ORACLE).expect("Go Copilot parser oracle");
    for case in fixture["cases"].as_array().expect("oracle cases") {
        let directory = tempfile::tempdir().expect("temporary Copilot directory");
        for (key, filename) in [("apps_json", "apps.json"), ("hosts_json", "hosts.json")] {
            if let Some(contents) = case[key].as_str() {
                write(directory.path(), filename, contents);
            }
        }
        let got = copilot_file_token_candidate_in(directory.path());
        let id = case["id"].as_str().expect("case id");
        if case["rust_candidate"] == true {
            let expected = case["token"].as_str().expect("Go token");
            let expected = (!expected.is_empty()).then_some(expected);
            assert_eq!(got.ok().flatten().as_deref(), expected, "Go case {id}");
            if case["native_route"] == true {
                assert_eq!(copilot_file_token_in(directory.path()).as_deref(), expected);
            }
        } else if case["native_route"] == true {
            assert!(
                got.expect("both Copilot files absent").is_none(),
                "Go case {id}"
            );
            assert!(copilot_file_token_in(directory.path()).is_none());
        } else {
            assert!(got.is_err(), "unproven Go case {id} must stay on Go");
        }
    }
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
fn literal_file_references_and_unproven_sources_keep_go_fallback() {
    assert!(!needs_go_fallback_for(&UsageFallbackSignals {
        copilot_file: Some("synthetic-copilot-file"),
        kimi_cli: Some("synthetic-kimi-file"),
        nous_file: Some("synthetic-nous-file"),
        codex_file: Some("synthetic-codex-file"),
        claude_file: Some("synthetic-claude-file"),
        ..UsageFallbackSignals::default()
    }));
    for reference in [
        "symvault://test/token",
        "vault://test/token",
        "env://TOKEN",
        "keychain://test/account",
    ] {
        for signals in [
            UsageFallbackSignals {
                copilot_file: Some(reference),
                ..UsageFallbackSignals::default()
            },
            UsageFallbackSignals {
                kimi_cli: Some(reference),
                ..UsageFallbackSignals::default()
            },
            UsageFallbackSignals {
                nous_file: Some(reference),
                ..UsageFallbackSignals::default()
            },
            UsageFallbackSignals {
                codex_file: Some(reference),
                ..UsageFallbackSignals::default()
            },
            UsageFallbackSignals {
                claude_file: Some(reference),
                ..UsageFallbackSignals::default()
            },
        ] {
            assert!(needs_go_fallback_for(&signals));
        }
    }
    for signals in [
        UsageFallbackSignals {
            other_provider_env: true,
            ..UsageFallbackSignals::default()
        },
        UsageFallbackSignals {
            other_credential_source: true,
            ..UsageFallbackSignals::default()
        },
        UsageFallbackSignals {
            local_provider_present: true,
            ..UsageFallbackSignals::default()
        },
    ] {
        assert!(needs_go_fallback_for(&signals));
    }
}

#[test]
fn claude_oauth_source_preempts_keychain_fallback() {
    for credential in [
        "synthetic-claude-oauth",
        "env://MISSING",
        "symvault://test/token",
    ] {
        assert!(!needs_go_fallback_for(&UsageFallbackSignals {
            claude_oauth_env: Some(credential),
            local_provider_present: true,
            ..UsageFallbackSignals::default()
        }));
    }
    assert!(!needs_go_fallback_for(&UsageFallbackSignals {
        claude_file: Some("synthetic-claude-file"),
        local_provider_present: true,
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
fn kimi_file_candidate_matches_only_the_source_bound_native_shapes() {
    let fixture: serde_json::Value =
        serde_json::from_str(KIMI_FILE_TOKEN_ORACLE).expect("Go Kimi file parser oracle");
    for case in fixture["cases"].as_array().expect("oracle cases") {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = write(
            directory.path(),
            "credentials/kimi-code.json",
            case["contents"].as_str().unwrap_or(""),
        );
        if !case["file_present"].as_bool().expect("file presence") {
            fs::remove_file(&path).expect("remove absent-file fixture");
        }
        let candidate = kimi_file_token_candidate(&path);
        if case["native_route"].as_bool().expect("native route") {
            assert_eq!(candidate.as_deref(), Some("synthetic-kimi-file-token"));
            assert_eq!(
                kimi_store(directory.path()).0.as_deref(),
                case["token"].as_str(),
                "production store follows Go for {}",
                case["id"]
            );
        } else {
            assert!(candidate.is_none(), "{} must remain on Go", case["id"]);
        }
    }
}

#[test]
fn nous_file_candidate_matches_source_bound_plain_and_live_jwt_tokens() {
    let fixture: serde_json::Value =
        serde_json::from_str(NOUS_FILE_TOKEN_ORACLE).expect("Go Nous file parser oracle");
    for case in fixture["cases"].as_array().expect("oracle cases") {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = write(
            directory.path(),
            "auth.json",
            case["contents"].as_str().unwrap_or(""),
        );
        if !case["file_present"].as_bool().expect("file presence") {
            fs::remove_file(&path).expect("remove absent-file fixture");
        }
        let candidate = nous_file_token_candidate(&path);
        if case["native_route"].as_bool().expect("native route") {
            assert_eq!(candidate.as_deref(), Some(case["token"].as_str().unwrap()));
            assert_eq!(nous_file_token(&path), candidate, "Go and Rust selection");
        } else {
            assert!(candidate.is_none(), "{} must remain on Go", case["id"]);
        }
    }
}

#[test]
fn supported_provider_overrides_are_narrow_and_go_equivalent() {
    for base in [
        "https://api.example.com/v1",
        "https://openrouter.ai/custom/v1/",
    ] {
        assert!(supported_custom_base(base), "{base}");
    }
    for base in [
        "http://api.example.com/v1",
        "https://localhost/v1",
        "https://api.example.com:8443/v1",
        "https://user@api.example.com/v1",
        "https://api.example.com/v1?next=x",
        "https://api.example.com/a/../v1",
        "https://api.example.com/a/./v1",
        "https://api.example.com/./v1",
        "https://api.example.com/%2fadmin",
    ] {
        assert!(!supported_custom_base(base), "{base} stays on Go");
    }
    assert!(supported_opencode_workspace("wrk_workspace123"));
    for workspace in [
        "",
        "workspace123",
        "wrk_",
        "wrk_a/b",
        "wrk_a?b",
        "symvault://workspace",
    ] {
        assert!(
            !supported_opencode_workspace(workspace),
            "{workspace:?} stays on Go"
        );
    }
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
    let now_seconds = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    assert!(!nous_jwt_is_live(&jwt_expiring_at(now_seconds)));
    let boundary_payload = encode_base64url(&format!(r#"{{"exp":{now_seconds}.9}}"#));
    assert!(!nous_jwt_is_live(&format!(
        "header.{boundary_payload}.signature"
    )));
    let fractional_payload = encode_base64url(r#"{"exp":4102444800.9,"sub":"synthetic"}"#);
    assert!(nous_jwt_is_live(&format!(
        "header.{fractional_payload}.signature"
    )));
    for ambiguous_payload in [
        r#"{"exp":4102444800,"EXP":0}"#,
        r#"{"EXP":0,"exp":4102444800}"#,
    ] {
        assert!(!nous_jwt_is_live(&format!(
            "header.{}.signature",
            encode_base64url(ambiguous_payload)
        )));
    }
    assert!(!nous_jwt_is_live("not-a-jwt"));
    assert!(!nous_jwt_is_live("only.two"));
    assert!(!nous_jwt_is_live("no.exp-claim.signature"));
    assert!(!nous_jwt_is_live(
        "header.eyJleHAiOjQxMDI0NDQ4MDB9=.signature"
    ));
    assert!(!nous_jwt_is_live(&format!(
        "header.{}.signature",
        encode_base64url(r#"{"exp":"4102444800"}"#)
    )));
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
fn claude_keychain_blob_parser_matches_go_typed_decoder_cases() {
    let oracle: serde_json::Value = serde_json::from_str(CLAUDE_KEYCHAIN_TOKEN_ORACLE)
        .expect("Go Claude keychain parser oracle");
    let cases = oracle["cases"].as_array().expect("keychain parser cases");
    assert!(
        cases.len() >= 20,
        "keychain parser fixture must retain edge cases"
    );

    for case in cases {
        let blob = if let Some(hex) = case["blob_hex"].as_str() {
            decode_fixture_hex(hex)
        } else {
            case["blob"]
                .as_str()
                .expect("keychain blob input")
                .as_bytes()
                .to_vec()
        };
        let parsed = parse_claude_keychain_blob(&blob);
        assert_eq!(parsed.is_some(), case["accepted"], "case {}", case["id"]);
        if let Some((token, expires_at)) = parsed {
            assert_eq!(
                token,
                case["token"].as_str().expect("Go token"),
                "case {}",
                case["id"]
            );
            let actual_millis = expires_at.map(|expiry| {
                i64::try_from(
                    expiry
                        .duration_since(std::time::UNIX_EPOCH)
                        .expect("positive fixture expiry")
                        .as_millis(),
                )
                .expect("expiry milliseconds fit i64")
            });
            assert_eq!(
                actual_millis,
                case["expires_at_unix_milli"].as_i64(),
                "expiry case {}",
                case["id"]
            );
        }
    }
}

fn decode_fixture_hex(hex: &str) -> Vec<u8> {
    let (pairs, remainder) = hex.as_bytes().as_chunks::<2>();
    assert!(remainder.is_empty(), "fixture hex has complete bytes");
    pairs
        .iter()
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => panic!("invalid fixture hex digit"),
            };
            digit(pair[0]) * 16 + digit(pair[1])
        })
        .collect()
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

#[test]
fn kimi_device_id_native_gate_accepts_only_absent_or_readable_ascii_files() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("device_id");
    assert!(
        kimi_device_id_is_native(&path),
        "missing file is equivalent"
    );
    fs::write(&path, b"device-42\n").expect("write ASCII device id");
    assert!(kimi_device_id_is_native(&path));
    fs::write(&path, b" device-42\t").expect("write trim-compatible device id");
    assert!(kimi_device_id_is_native(&path));
    fs::write(&path, b"device\nid").expect("write embedded newline");
    assert!(!kimi_device_id_is_native(&path));
    fs::write(&path, b"device\0id").expect("write embedded NUL");
    assert!(!kimi_device_id_is_native(&path));
    fs::write(&path, b"").expect("write empty device id");
    assert!(kimi_device_id_is_native(&path));
    fs::write(&path, [0xff]).expect("write invalid UTF-8");
    assert!(!kimi_device_id_is_native(&path));
    fs::write(
        &path,
        vec![b'x'; usize::try_from(MAX_CREDENTIAL_FILE_BYTES).expect("file size fits usize") + 1],
    )
    .expect("write oversized device id");
    assert!(!kimi_device_id_is_native(&path));

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        fs::remove_file(&path).expect("remove device id");
        let target = directory.path().join("target");
        fs::write(&target, b"device-42").expect("write symlink target");
        symlink(target, &path).expect("create device id symlink");
        assert!(!kimi_device_id_is_native(&path));
    }
}
