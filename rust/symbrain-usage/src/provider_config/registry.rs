// Native provider registry compatibility implementation.
// ---------------------------------------------------------------------------
// Antigravity process probe
// ---------------------------------------------------------------------------

fn antigravity_process_list_running(process_list: &str) -> bool {
    process_list.contains("agy") || process_list.contains("Antigravity")
}

// ---------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------

fn auth_error(detail: &str) -> AuthStatus {
    AuthStatus {
        status: "missing".into(),
        detail: format!("credential resolution failed: {detail}"),
        source: Some("vault".into()),
    }
}

fn missing(detail: &str) -> AuthStatus {
    AuthStatus {
        status: "missing".into(),
        detail: detail.into(),
        source: None,
    }
}

fn available(detail: &str, source: String) -> AuthStatus {
    AuthStatus {
        status: "available".into(),
        detail: detail.into(),
        source: Some(source),
    }
}

#[must_use]
pub fn all_providers() -> Vec<Provider> {
    vec![
        claude(),
        codex(),
        copilot(),
        cursor(),
        kimi(),
        moonshot(),
        nous(),
        opencode(),
        openrouter(),
        antigravity(),
    ]
}
