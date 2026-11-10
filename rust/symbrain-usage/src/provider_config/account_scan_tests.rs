// Read-only discovery against fabricated homes: no credential value may reach
// the output, and no store is written.
use std::{fs, path::Path};

use super::{claude_cli_account_id, claude_desktop_account_id, codex_file_account_id};
use crate::accounts::{ClaudeScan, CodexScan, claude_accounts, codex_accounts};

const SECRETS: [&str; 10] = [
    "sk-ant-oat01-SECRET-ACCESS",
    "sk-ant-ort01-SECRET-REFRESH",
    "codex-access-SECRET",
    "codex-refresh-SECRET",
    "someone@example.com",
    "Some Person",
    "11111111",
    "22222222",
    "cdx-0001",
    "SECRET",
];

fn write(dir: &Path, rel: &str, body: &str) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

fn jwt(claims: &str) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    format!("e30.{}.sig", b64.encode(claims))
}

fn fabricated_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    write(
        home.path(),
        ".claude.json",
        r#"{"projects":{},"oauthAccount":{"accountUuid":"11111111-aaaa-4aaa-8aaa-111111111111",
        "emailAddress":"someone@example.com","fullName":"Some Person","organizationName":"Some Person"}}"#,
    );
    write(
        home.path(),
        ".claude/.credentials.json",
        r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-SECRET-ACCESS","refreshToken":"sk-ant-ort01-SECRET-REFRESH"}}"#,
    );
    write(
        home.path(),
        "Library/Application Support/Claude/config.json",
        r#"{"lastKnownAccountUuid":"22222222-bbbb-4bbb-8bbb-222222222222","oauth:tokenCacheV2":"SECRET-CIPHERTEXT"}"#,
    );
    let id_token = jwt(
        r#"{"email":"someone@example.com","https://api.openai.com/auth":{"chatgpt_account_id":"cdx-0001"}}"#,
    );
    write(
        home.path(),
        ".codex/auth.json",
        &format!(
            r#"{{"tokens":{{"access_token":"codex-access-SECRET","refresh_token":"codex-refresh-SECRET","id_token":"{id_token}"}}}}"#
        ),
    );
    home
}

#[test]
fn fabricated_home_discovers_ids_and_output_carries_no_credential_material() {
    let home = fabricated_home();
    let before = fs::read(home.path().join(".codex/auth.json")).unwrap();
    let cli = claude_cli_account_id(home.path());
    let desktop = claude_desktop_account_id(home.path());
    let codex = codex_file_account_id(&home.path().join(".codex"));
    assert_eq!(cli.as_deref(), Some("11111111-aaaa-4aaa-8aaa-111111111111"));
    assert_eq!(
        desktop.as_deref(),
        Some("22222222-bbbb-4bbb-8bbb-222222222222")
    );
    assert_eq!(
        codex.as_deref(),
        Some("cdx-0001"),
        "id_token claim fallback"
    );

    let claude = claude_accounts(&ClaudeScan {
        oauth_source: Some("file".into()),
        cli_account_id: cli,
        desktop_account_id: desktop,
        ..ClaudeScan::default()
    });
    let codex = codex_accounts(&CodexScan {
        token_source: Some("env".into()),
        file_account_id: codex,
    });
    assert_eq!((claude.len(), codex.len()), (2, 2));
    let json = serde_json::to_string(&(claude, codex)).unwrap();
    for secret in SECRETS {
        assert!(!json.contains(secret), "{secret} leaked into {json}");
    }
    assert_eq!(
        fs::read(home.path().join(".codex/auth.json")).unwrap(),
        before
    );
}

#[test]
fn missing_or_malformed_account_files_yield_no_account() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(claude_cli_account_id(home.path()), None);
    assert_eq!(codex_file_account_id(home.path()), None);
    write(home.path(), ".claude.json", "{not json");
    write(
        home.path(),
        "auth.json",
        r#"{"tokens":{"account_id":"  "}}"#,
    );
    assert_eq!(claude_cli_account_id(home.path()), None);
    assert_eq!(codex_file_account_id(home.path()), None);
}
