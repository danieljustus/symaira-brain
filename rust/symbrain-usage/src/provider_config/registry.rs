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

/// Validated provider IDs in stable catalog order; parsing performs no I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSelection(Vec<&'static str>);

type ProviderFactory = (&'static str, fn() -> Provider);
const PROVIDER_FACTORIES: [ProviderFactory; 14] = [
    ("claude", claude),
    ("codex", codex),
    ("copilot", copilot),
    ("cursor", cursor),
    ("kimi", kimi),
    ("moonshot", moonshot),
    ("nous", nous),
    ("opencode", opencode),
    ("openrouter", openrouter),
    ("antigravity", antigravity),
    ("zai", zai),
    ("devin", devin),
    ("grok", grok),
    ("ollama", Provider::ollama),
];

impl Default for ProviderSelection {
    fn default() -> Self {
        Self(PROVIDER_FACTORIES[..10].iter().map(|(id, _)| *id).collect())
    }
}

impl ProviderSelection {
    /// # Errors
    /// Rejects empty or unknown IDs before any credential discovery.
    pub fn parse(selector: &str) -> Result<Self, String> {
        let ids: Vec<_> = selector.split(',').collect();
        for id in &ids {
            if !PROVIDER_FACTORIES.iter().any(|(known, _)| known == id) {
                return Err(format!("invalid provider ID {id:?}"));
            }
        }
        Ok(Self(
            PROVIDER_FACTORIES
                .iter()
                .filter_map(|(id, _)| ids.contains(id).then_some(*id))
                .collect(),
        ))
    }

    fn contains(&self, id: &str) -> bool {
        self.0.contains(&id)
    }
}

#[must_use]
pub fn all_providers() -> Vec<Provider> {
    selected_providers(&ProviderSelection::default())
}

#[must_use]
pub fn selected_providers(selection: &ProviderSelection) -> Vec<Provider> {
    PROVIDER_FACTORIES
        .iter()
        .filter(|(id, _)| selection.contains(id))
        .map(|(_, constructor)| constructor())
        .collect()
}
