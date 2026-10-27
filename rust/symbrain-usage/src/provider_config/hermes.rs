// Read-only, capability-rooted Hermes credential decoder.
#[test]
fn current_hermes_store() {
    let token = decode_hermes_token(br#"{"providers":{"nous":{"access_token":"fixture-portal"}}}"#)
        .unwrap();
    let provider = nous_from_resolved(
        Some(("file".into(), token)),
        None,
        "https://portal.nousresearch.com".into(),
    );
    let transport = std::sync::Arc::new(crate::FixtureTransport::new(
        std::collections::BTreeMap::from([(
            "nous".into(),
            crate::Response {
                status: 200,
                headers: std::collections::BTreeMap::new(),
                body: br#"{"paid_service_access":{"total_usable_credits":42}}"#.to_vec(),
            },
        )]),
    ));
    let report = crate::Service::with_transport(vec![provider], transport.clone()).report();
    assert!(report.providers[0].configured);
    assert!(report.providers[0].error.is_none());
    assert_eq!(
        report.providers[0]
            .snapshot
            .as_ref()
            .unwrap()
            .balance
            .as_deref(),
        Some("42")
    );
    assert!(
        transport.requests()[0]
            .headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("authorization")
                && value == "Bearer fixture-portal")
    );
    assert_eq!(decode_hermes_token(br#"{"providers":{"nous":{"access_token":"fixture-stale","last_auth_error":{"message":"private diagnostic"}}},"credential_pool":{"nous":[{"access_token":"fixture-other"}]}}"#), None);
    assert_eq!(decode_hermes_token(br#"{"providers":{"nous":{"access_token":"fixture-portal","agent_key":"fixture-inference","invoke_jwt":"fixture-invoke"}}}"#), Some("fixture-portal".into()));
    assert_eq!(
        decode_hermes_token(br#"{"providers":{"nous":{"access_token":"fixture-portal"}}}"#),
        Some("fixture-portal".into())
    );
    assert_eq!(
        decode_hermes_token(br#"{"credential_pool":{"nous":[{"access_token":"fixture-pool"}]}}"#),
        Some("fixture-pool".into())
    );
    assert_eq!(decode_hermes_token(br#"{"providers":{"nous":{"access_token":"fixture-old","expires_at":"2000-01-01T00:00:00Z"}}}"#), None);
}
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
    if !go_json_credential_limits(&text, false) {
        return None;
    }
    let root: CredentialFields = serde_json::from_str(&text).ok()?;
    let current_providers = root
        .0
        .iter()
        .rev()
        .find(|(name, _)| go_json_field_matches(name, "providers"))
        .filter(|(_, raw)| raw.get().starts_with('{'));
    if let Some((_, raw)) = current_providers {
        let providers: serde_json::Value = serde_json::from_str(raw.get()).ok()?;
        if let Some(state) = providers.get("nous") {
            return current_nous_state_token(state);
        }
    }
    if let Some((_, raw)) = root
        .0
        .iter()
        .rev()
        .find(|(name, _)| name == "credential_pool")
    {
        let pool: serde_json::Value = serde_json::from_str(raw.get()).ok()?;
        if let Some(entries) = pool.get("nous").and_then(serde_json::Value::as_array) {
            return entries.iter().find_map(current_nous_state_token);
        }
    }
    if current_providers.is_some() {
        return None;
    }
    // Go's decoder reuses the slice backing array across duplicate fields.
    // Keep visited slots separate from visible length: shrinking a nonempty
    // array hides its tail without destroying values reused by later growth.
    let mut providers: Vec<HermesProvider> = Vec::new();
    let mut visible_length = 0;
    for (name, value) in root.0 {
        if !go_json_field_matches(&name, "providers") {
            continue;
        }
        if value.get() == "null" {
            providers.clear();
            visible_length = 0;
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
        visible_length = length;
        // Go replaces [] with a new empty slice, unlike nonempty shrinkage.
        if length == 0 {
            providers.clear();
        }
    }
    let provider = providers
        .into_iter()
        .take(visible_length)
        .find(|provider| provider.id == "nous")?;
    let token = if provider.invoke_jwt.is_empty() {
        provider.access_token
    } else {
        provider.invoke_jwt
    };
    (!token.is_empty()).then_some(token)
}

fn current_nous_state_token(state: &serde_json::Value) -> Option<String> {
    if state
        .get("last_auth_error")
        .is_some_and(|error| !error.is_null())
    {
        return None;
    }
    if let Some(expiry) = state.get("expires_at").and_then(serde_json::Value::as_str)
        && !expiry.is_empty()
    {
        let expiry = chrono::DateTime::parse_from_rfc3339(expiry).ok()?;
        if expiry <= chrono::Utc::now() {
            return None;
        }
    }
    let token = state
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .or_else(|| state.get("invoke_jwt").and_then(serde_json::Value::as_str))?;
    (!token.is_empty() && (!token.contains('.') || nous_jwt_is_live(token))).then(|| token.into())
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
