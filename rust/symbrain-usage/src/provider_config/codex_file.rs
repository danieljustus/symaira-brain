// Generic Go map decoding: exact keys and last duplicate value, without a
// recursive Value tree or accidental limits on ignored metadata depth.
fn codex_file_token(home_dir: &Path) -> Option<String> {
    let text = go_json_compatible_text(&read_provider_credentials(&home_dir.join("auth.json"))?);
    let root: CredentialFields = serde_json::from_str(&text).ok()?;
    if !go_json_credential_limits(&text, true) {
        return None;
    }
    let mut access = None;
    let mut tokens = None;
    for (name, value) in root.0 {
        match name.as_str() {
            "access_token" => access = serde_json::from_str::<String>(value.get()).ok(),
            "tokens" => tokens = Some(value),
            _ => {}
        }
    }
    if let Some(token) = access.filter(|token| !token.is_empty()) {
        return Some(token);
    }
    let entries: CredentialFields = serde_json::from_str(tokens?.get()).ok()?;
    let mut token = None;
    for (name, value) in entries.0 {
        if name == "access_token" {
            token = serde_json::from_str::<String>(value.get()).ok();
        }
    }
    token.filter(|token| !token.is_empty())
}
