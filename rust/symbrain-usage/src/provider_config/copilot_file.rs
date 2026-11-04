// Go root maps replace duplicate entries; typed entry fields merge in order.
#[test]
fn current_copilot_selected_account() {
    assert_eq!(copilot_cli_account(br#"{// comment
      "loggedInUsers":[{"host":"https://github.com","login":"first"},{"host":"https://github.com","login":"second"}],
      "lastLoggedInUser":{"host":"https://github.com","login":"second"},
    }"#).as_deref(), Some("https://github.com:second"));
}

// Go's reader treats any unreadable, unsafe, oversized or malformed file as
// absent and continues with the next file. For several distinct eligible
// tokens Go's unordered map iteration picks any of them; the sorted map makes
// the native choice deterministic.
fn copilot_file_token_in(dir: &Path) -> Option<String> {
    for name in ["apps.json", "hosts.json"] {
        let Some(contents) = read_provider_credentials(&credential_join(dir, name)) else {
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
            let selected = entries
                .iter()
                .filter(|(key, _)| key.starts_with(prefix))
                .find_map(|(_, raw)| decode_typed_credential(raw.get(), "oauth_token"));
            if selected.is_some() {
                return selected;
            }
        }
    }
    None
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
