// Typed Claude file decoding, preserving Go map merging and scalar nulls.
fn claude_file_token() -> Option<String> {
    claude_file_token_in(&home().join(".claude/.credentials.json"))
}

fn claude_accounts(contents: &[u8]) -> Option<std::collections::BTreeMap<String, String>> {
    let text = go_json_compatible_text(contents);
    if !go_json_credential_limits(&text, false) {
        return None;
    }
    let root: CredentialFields = serde_json::from_str(&text).ok()?;
    let mut accounts = std::collections::BTreeMap::new();
    for (name, value) in root.0 {
        if !go_json_field_matches(&name, "oauthAccount") {
            continue;
        }
        if value.get() == "null" {
            accounts.clear();
            continue;
        }
        let entries: CredentialFields = serde_json::from_str(value.get()).ok()?;
        for (account, value) in entries.0 {
            // Go allocates a fresh map value for each entry, while repeated
            // root objects merge into the existing map. A null entry resets
            // its value, but null accessToken leaves its scalar unchanged.
            let fields: CredentialFields = serde_json::from_str(value.get()).ok()?;
            let mut token = String::new();
            for (name, value) in fields.0 {
                if go_json_field_matches(&name, "accessToken") && value.get() != "null" {
                    token = serde_json::from_str(value.get()).ok()?;
                }
            }
            accounts.insert(account, token);
        }
    }
    Some(accounts)
}

fn claude_file_token_in(path: &Path) -> Option<String> {
    let accounts = claude_accounts(&read_provider_credentials(path)?)?;
    claude_selected_token(&accounts)
}

// Go prefers a nonempty `default` account, then takes whichever nonempty token
// its unordered map iteration yields first. Any nonempty token is therefore a
// Go-possible outcome; the sorted map makes the native choice deterministic.
fn claude_selected_token(accounts: &std::collections::BTreeMap<String, String>) -> Option<String> {
    accounts
        .get("default")
        .filter(|token| !token.is_empty())
        .or_else(|| accounts.values().find(|token| !token.is_empty()))
        .cloned()
}
