use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageError {
    Transport {
        provider: String,
        detail: String,
    },
    Status {
        provider: String,
        status: u16,
        retry_after: Option<String>,
        detail: String,
    },
    Parse {
        provider: String,
        detail: String,
    },
    Chain {
        provider: String,
        failures: Vec<String>,
    },
    NotRunning,
    Exact(String),
}
impl UsageError {
    pub(crate) fn transport(provider: &str, detail: &str) -> Self {
        Self::Transport {
            provider: provider.into(),
            detail: redact(detail),
        }
    }
    pub(crate) fn parse(provider: &str, detail: &str) -> Self {
        Self::Parse {
            provider: provider.into(),
            detail: detail.into(),
        }
    }
    pub(crate) fn chain(provider: &str, errors: Vec<Self>) -> Self {
        Self::Chain {
            provider: provider.into(),
            failures: errors.into_iter().map(|e| e.to_string()).collect(),
        }
    }
    pub(crate) fn not_running() -> Self {
        Self::NotRunning
    }
    pub(crate) fn exact(text: impl Into<String>) -> Self {
        Self::Exact(text.into())
    }
}
impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport { provider, detail } => {
                write!(f, "{provider} request failed: {detail}")
            }
            Self::Status { detail, .. } if !detail.is_empty() => write!(f, "{detail}"),
            Self::Status {
                provider, status, ..
            } => {
                write!(f, "{provider} request failed with HTTP {status}")
            }
            Self::Parse { provider, detail } if provider == "antigravity" => {
                write!(f, "Antigravity returned an unreadable response: {detail}")
            }
            Self::Parse { provider, detail } if provider == "opencode" => {
                write!(f, "OpenCode returned an unreadable response: {detail}")
            }
            Self::Parse { provider, detail } => write!(
                f,
                "AI usage provider \"{provider}\" returned malformed usage data: {detail}"
            ),
            Self::Chain { failures, .. } => {
                write!(f, "all AI usage fallbacks failed: {}", failures.join("; "))
            }
            Self::NotRunning => write!(
                f,
                "Antigravity is not running — no local quota server found."
            ),
            Self::Exact(text) => f.write_str(text),
        }
    }
}
impl std::error::Error for UsageError {}

pub(super) fn status_error(
    provider: &str,
    status: u16,
    headers: &BTreeMap<String, String>,
) -> UsageError {
    let retry = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("retry-after"))
        .map(|(_, value)| value.clone());
    let retry_text = retry
        .as_deref()
        .map(|value| format!("; retry in {value}s"))
        .unwrap_or_default();
    let detail = match (provider, status) {
        ("claude", 401 | 403) => {
            format!("Claude rejected the login (HTTP {status}). Re-auth or switch the usage source.")
        }
        ("copilot", 401 | 403) => {
            format!("Copilot rejected the login (HTTP {status}). Re-authenticate with GitHub Copilot.")
        }
        ("cursor", 401 | 403) => {
            "Cursor session is invalid or expired. Re-import the cursor.com cookie or sign in to Cursor.app.".into()
        }
        ("kimi", 401 | 403) => {
            format!("Kimi rejected the login (HTTP {status}). Check the API access or sign in with the Kimi Code CLI again.")
        }
        ("opencode", 401 | 403) => {
            "OpenCode session cookie is invalid or expired. Re-import the opencode.ai cookie.".into()
        }
        ("antigravity", _) => format!("Antigravity local server returned HTTP {status}."),
        (_, 401 | 403) => format!("AI usage provider \"{provider}\" is not configured"),
        ("opencode", 429) => "OpenCode API error: HTTP 429".into(),
        ("claude" | "codex" | "copilot" | "cursor" | "kimi" | "moonshot" | "nous" | "openrouter", 429) => {
            format!("AI usage provider \"{provider}\" is rate limited{retry_text}")
        }
        _ => format!("{provider} request failed with HTTP {status}"),
    };
    UsageError::Status {
        provider: provider.into(),
        status,
        retry_after: retry,
        detail,
    }
}
fn redact(value: &str) -> String {
    if value.contains("token") || value.contains("secret") || value.contains("Bearer") {
        "redacted provider transport error".into()
    } else {
        value.chars().take(256).collect()
    }
}
