// Read-only discovery of local Claude and Codex accounts (#1000). Only key
// names and opaque ids are read; tokens and e-mail addresses are never
// deserialized, so they cannot reach the report.

/// Account files may be large (`~/.claude.json` carries every project).
const MAX_ACCOUNT_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// Service the real keychain reader took the Claude token from; handed over once
/// like the plan so the keychain is never read twice.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
static CLAUDE_KEYCHAIN_SERVICE_USED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn remember_claude_keychain_service(service: &str) {
    if let Ok(mut slot) = CLAUDE_KEYCHAIN_SERVICE_USED.lock() {
        *slot = Some(service.to_owned());
    }
}

fn take_claude_keychain_service() -> Option<String> {
    CLAUDE_KEYCHAIN_SERVICE_USED
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
}

fn read_account_file(path: &Path) -> Option<Vec<u8>> {
    let file = fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_ACCOUNT_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_ACCOUNT_FILE_BYTES).then_some(bytes)
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

/// `oauthAccount.accountUuid` of `~/.claude.json`, the default Claude Code login.
fn claude_cli_account_id(home: &Path) -> Option<String> {
    #[derive(Deserialize)]
    struct Root {
        #[serde(rename = "oauthAccount")]
        account: Option<Account>,
    }
    #[derive(Deserialize)]
    struct Account {
        #[serde(rename = "accountUuid")]
        id: Option<String>,
    }
    let root: Root = serde_json::from_slice(&read_account_file(&home.join(".claude.json"))?).ok()?;
    non_empty(root.account?.id)
}

/// `lastKnownAccountUuid` of Claude Desktop's `config.json`; its tokens are
/// encrypted with a Safe Storage key and are deliberately not read.
fn claude_desktop_account_id(home: &Path) -> Option<String> {
    #[derive(Deserialize)]
    struct Root {
        #[serde(rename = "lastKnownAccountUuid")]
        id: Option<String>,
    }
    let path = home.join("Library/Application Support/Claude/config.json");
    let root: Root = serde_json::from_slice(&read_account_file(&path)?).ok()?;
    non_empty(root.id)
}

/// Account id of the Codex login in `<codex home>/auth.json`: `tokens.account_id`,
/// else the `chatgpt_account_id` claim of the ID token.
fn codex_file_account_id(codex_home: &Path) -> Option<String> {
    #[derive(Deserialize)]
    struct Root {
        tokens: Option<Tokens>,
    }
    #[derive(Deserialize)]
    struct Tokens {
        account_id: Option<String>,
        id_token: Option<String>,
    }
    let root: Root =
        serde_json::from_slice(&read_account_file(&codex_home.join("auth.json"))?).ok()?;
    let tokens = root.tokens?;
    non_empty(tokens.account_id).or_else(|| {
        let payload = tokens.id_token?.split('.').nth(1).map(decode_base64url)??;
        let claims: serde_json::Value = serde_json::from_slice(&payload).ok()?;
        let id = claims
            .get("https://api.openai.com/auth")?
            .get("chatgpt_account_id")?
            .as_str()?;
        non_empty(Some(id.to_owned()))
    })
}

#[cfg(target_os = "macos")]
fn claude_keychain_service_names() -> Vec<String> {
    let Ok(listed) = keychain_dump_names("/usr/bin/security", MAX_KEYCHAIN_LISTING_BYTES) else {
        return Vec::new();
    };
    let mut names: Vec<String> = listed
        .into_iter()
        .filter(|name| valid_claude_service_name(name))
        .collect();
    names.sort();
    names.dedup();
    names
}

#[cfg(not(target_os = "macos"))]
fn claude_keychain_service_names() -> Vec<String> {
    Vec::new()
}

/// `service` is set only by the real keychain reader, so an injected reader
/// (tests) never triggers a keychain listing.
fn claude_account_scan(
    oauth_source: Option<String>,
    admin_key: bool,
    service: Option<String>,
) -> crate::accounts::ClaudeScan {
    let home = home();
    crate::accounts::ClaudeScan {
        keychain_services: if service.is_some() {
            claude_keychain_service_names()
        } else {
            Vec::new()
        },
        keychain_service: service,
        oauth_source,
        admin_key,
        custom_config_dir: env::var_os("CLAUDE_CONFIG_DIR").is_some_and(|v| !v.is_empty()),
        cli_account_id: claude_cli_account_id(&home),
        desktop_account_id: claude_desktop_account_id(&home),
    }
}
