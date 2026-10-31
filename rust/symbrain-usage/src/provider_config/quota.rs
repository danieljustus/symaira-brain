// Explicit environment values (including failed secret references) preempt files.
fn devin() -> Provider {
    let result = resolve_env_or_file("DEVIN_API_KEY", || {
        devin_file_key(&home().join(".local/share/devin/credentials.toml"))
    });
    let mut provider = quota_from_resolved("devin", "Devin", "DEVIN_API_KEY", result);
    provider.spec.credential_sources.push("credentials.toml");
    if provider.configured {
        provider.auth_status.detail = "Devin credential configured".into();
    } else if provider.auth_status.source.is_none() {
        provider.auth_status.detail =
            "no Devin credential (DEVIN_API_KEY or Devin CLI credentials.toml)".into();
    }
    provider
}
fn devin_file_key(path: &Path) -> Option<String> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() {
        return None;
    }
    let text = String::from_utf8(read_limited(path)?).ok()?;
    let doc: toml_edit::DocumentMut = text.parse().ok()?;
    // api_server_url is intentionally ignored; credentials only go to server.codeium.com.
    let key = doc.get("windsurf_api_key")?.as_str()?.trim();
    if key.is_empty() || key.contains(['\r', '\n', '\0']) {
        return None;
    }
    Some(key.into())
}
// Only explicit environment credentials are read for Z.ai.
fn zai() -> Provider {
    quota_from_resolved("zai", "Z.ai", "ZAI_API_KEY", resolve_env("ZAI_API_KEY"))
}
fn quota_from_resolved(
    id: &str,
    name: &str,
    variable: &'static str,
    result: Result<Option<(String, String)>, String>,
) -> Provider {
    let (credentials, status) = match result {
        Ok(Some((source, key))) if !key.is_empty() => (
            vec![(source.clone(), key)],
            available(&format!("API key configured ({variable})"), source),
        ),
        Ok(_) => (
            Vec::new(),
            missing(&format!("no API key configured ({variable})")),
        ),
        Err(_) => (
            Vec::new(),
            AuthStatus {
                status: "missing".into(),
                detail: "credential resolution failed".into(),
                source: Some("vault".into()),
            },
        ),
    };
    Provider::new(id, name, vec![variable], credentials, status)
}
