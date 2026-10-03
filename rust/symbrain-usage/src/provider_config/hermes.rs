// Read-only, capability-rooted Hermes credential decoder.
fn nous_auth_path() -> PathBuf {
    env_path("HERMES_HOME", home().join(".hermes")).join("auth.json")
}

#[derive(Default)]
struct HermesProvider {
    id: String,
    invoke_jwt: String,
    access_token: String,
}

fn decode_hermes_token(contents: &[u8]) -> Option<String> {
    let text = go_json_compatible_text(contents);
    let root: CredentialFields = serde_json::from_str(&text).ok()?;
    let mut providers: Vec<HermesProvider> = Vec::new();
    for (name, value) in root.0 {
        if !go_json_field_matches(&name, "providers") {
            continue;
        }
        if value.get() == "null" {
            providers.clear();
            continue;
        }
        let entries: Vec<Box<serde_json::value::RawValue>> =
            serde_json::from_str(value.get()).ok()?;
        let length = entries.len();
        while providers.len() < length {
            providers.push(HermesProvider::default());
        }
        for (index, entry) in entries.into_iter().enumerate() {
            let fields: CredentialFields = serde_json::from_str(entry.get()).ok()?;
            for (name, value) in fields.0 {
                let field = if go_json_field_matches(&name, "id") {
                    &mut providers[index].id
                } else if go_json_field_matches(&name, "invoke_jwt") {
                    &mut providers[index].invoke_jwt
                } else if go_json_field_matches(&name, "access_token") {
                    &mut providers[index].access_token
                } else {
                    continue;
                };
                // Go null leaves non-pointer scalar fields unchanged.
                if value.get() != "null" {
                    *field = serde_json::from_str(value.get()).ok()?;
                }
            }
        }
        providers.truncate(length);
    }
    let provider = providers
        .into_iter()
        .find(|provider| provider.id == "nous")?;
    let token = if provider.invoke_jwt.is_empty() {
        provider.access_token
    } else {
        provider.invoke_jwt
    };
    (!token.is_empty()).then_some(token)
}

fn nous_file_token(path: &Path) -> Option<String> {
    let token = decode_hermes_token(&read_provider_credentials(path)?)?;
    (!token.contains('.') || nous_jwt_is_live(&token)).then_some(token)
}

fn nous_file_requires_go(path: &Path) -> bool {
    let Some(token) =
        read_provider_credentials(path).and_then(|contents| decode_hermes_token(&contents))
    else {
        return false;
    };
    jwt_expiry(&token).is_some_and(jwt_expiry_out_of_range)
}
