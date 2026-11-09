use super::{
    MAX_CREDENTIAL_FILE_BYTES, claude_file_token_in, codex_file_token, codex_from_resolved,
    copilot_file_token_in, decode_base64url, go_trim_space, json_string, kimi_store,
    names_from_keychain_dump, nous_file_token, nous_jwt_is_live, parse_claude_keychain_blob,
    read_limited,
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
        if let Some(possible) = case["possible_tokens"].as_array() {
            // Go's map iteration order is unspecified: any possible token is
            // a Go outcome; the native choice is the first in sorted key order.
            let got = got.unwrap_or_default();
            assert!(
                possible
                    .iter()
                    .any(|token| token.as_str() == Some(got.as_str())),
                "ambiguous account selection {id}: {got}"
            );
        } else {
            assert_eq!(
                got.unwrap_or_default(),
                case["token"].as_str().unwrap(),
                "Go case {id}"
            );
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
        let got = copilot_file_token_in(directory.path());
        let id = case["id"].as_str().expect("case id");
        if let Some(possible) = case["possible_tokens"].as_array() {
            // Go's map iteration order is unspecified: any possible token is
            // a Go outcome; the native choice is the first in sorted key order.
            let got = got.unwrap_or_default();
            assert!(
                possible
                    .iter()
                    .any(|token| token.as_str() == Some(got.as_str())),
                "distinct Go map tokens {id}: {got}"
            );
        } else {
            let expected = case["token"].as_str().unwrap();
            let expected = (!expected.is_empty()).then_some(expected);
            assert_eq!(got.as_deref(), expected, "Go case {id}");
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
        r#"{"github.com:owned":{"oauth_token":"github-token"},"other.com":{"oauth_token":"other-token"}}"#,
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
        (
            Some("kimi-token".to_owned()),
            Some("device-42".to_owned()),
            None
        )
    );
}

#[test]
fn kimi_file_token_matches_every_frozen_go_shape() {
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
        let expected = case["token"].as_str().unwrap();
        let expected = (!expected.is_empty()).then_some(expected);
        assert_eq!(
            kimi_store(directory.path()).0.as_deref(),
            expected,
            "{}",
            case["id"]
        );
    }
}

#[test]
fn nous_file_parser_matches_all_frozen_go_shapes() {
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
        assert_eq!(
            nous_file_token(&path).unwrap_or_default(),
            case["token"].as_str().expect("Go token"),
            "{}",
            case["id"]
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
    assert!(!nous_jwt_is_live(&format!(
        "header.{}.signature",
        encode_base64url(r#"{"exp":4102444800,"EXP":0}"#)
    )));
    assert!(nous_jwt_is_live(&format!(
        "header.{}.signature",
        encode_base64url(r#"{"EXP":0,"exp":4102444800}"#)
    )));
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
fn kimi_device_id_trims_like_go_and_keeps_invalid_utf8_bytes() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("device_id");
    let home = directory.path().join("cli-home");
    fs::create_dir_all(&home).expect("cli home");
    for (contents, expected) in [
        (&b"device-42\n"[..], Some(&b"device-42"[..])),
        (b" \tdevice 42\t ", Some(b"device 42")),
        (b"\xc2\xa0dev\xe2\x80\xa8", Some(b"dev")),
        (b"dev\xff", Some(b"dev\xff")),
        (b"\xff \n", Some(b"\xff")),
        (b"dev\x01ice", Some(b"dev\x01ice")),
        (b" \n\t", None),
        (b"", None),
    ] {
        fs::write(&path, contents).expect("device id");
        fs::copy(&path, home.join("device_id")).expect("copy");
        assert_eq!(
            super::kimi_device_id_bytes(&home).as_deref(),
            expected,
            "{contents:?}"
        );
    }
    assert_eq!(go_trim_space(b"\xc2\xa0 x \xe3\x80\x80"), b"x");
}
