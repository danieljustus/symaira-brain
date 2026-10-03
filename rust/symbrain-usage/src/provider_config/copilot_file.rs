// Go root maps replace duplicate entries; typed entry fields merge in order.
fn copilot_file_token_candidate_in(dir: &Path) -> Result<Option<String>, ()> {
    for name in ["apps.json", "hosts.json"] {
        let Some(contents) = read_optional_credential_file(&credential_join(dir, name))? else {
            continue;
        };
        let text = go_json_compatible_text(&contents);
        if !go_json_credential_limits(&text, false) {
            continue;
        }
        let Ok(root) = serde_json::from_str::<CredentialFields>(&text) else {
            continue;
        };
        let mut entries = std::collections::BTreeMap::new();
        for (key, value) in root.0 {
            entries.insert(key, value);
        }
        for prefix in ["github.com:", ""] {
            let mut selected = None;
            for (key, raw) in &entries {
                if !prefix.is_empty() && !key.starts_with(prefix) {
                    continue;
                }
                let Some(token) = decode_typed_credential(raw.get(), "oauth_token") else {
                    continue;
                };
                if selected.as_ref().is_some_and(|previous| previous != &token) {
                    // Different eligible tokens depend on Go map iteration.
                    return Err(());
                }
                selected = Some(token);
            }
            if selected.is_some() {
                return Ok(selected);
            }
        }
    }
    Ok(None)
}

// Scalar null retains prior typed string values. A wrong known type invalidates
// the entire typed object even if a later duplicate has a valid string.
fn decode_typed_credential(text: &str, name: &str) -> Option<String> {
    let fields: CredentialFields = serde_json::from_str(text).ok()?;
    let mut token = String::new();
    for (key, value) in fields.0 {
        if go_json_field_matches(&key, name) && value.get() != "null" {
            token = serde_json::from_str(value.get()).ok()?;
        }
    }
    (!token.is_empty()).then_some(token)
}
