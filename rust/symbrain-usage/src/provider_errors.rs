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
    /// A parseable response that carried no usable usage data (the shipped
    /// `PayloadError`).
    Payload {
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
    /// The shipped `PayloadError`: a 2xx body that parsed but carried no
    /// usable usage data.
    pub(crate) fn payload(provider: &str, detail: &str) -> Self {
        Self::Payload {
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
            Self::Transport { detail, .. } => {
                write!(f, "AI usage request failed: {detail}")
            }
            Self::Status { detail, .. } if !detail.is_empty() => write!(f, "{detail}"),
            Self::Status {
                provider, status, ..
            } => match error_name(provider) {
                // OpenCode reports every non-2xx through its own text.
                Some(_) if provider == "opencode" => {
                    write!(f, "OpenCode API error: HTTP {status}")
                }
                Some(name) => write!(f, "{name} request failed with HTTP {status}"),
                None => write!(f, "AI usage request failed with HTTP {status}"),
            },
            Self::Parse { provider, detail } if provider == "antigravity" => {
                write!(f, "Antigravity returned an unreadable response: {detail}")
            }
            Self::Parse { provider, detail } if provider == "opencode" => {
                write!(f, "OpenCode returned an unreadable response: {detail}")
            }
            Self::Parse { provider, detail } if provider == "cursor" => {
                write!(f, "Cursor returned an unreadable response: {detail}")
            }
            Self::Parse { provider, .. } if error_name(provider).is_some() => {
                write!(
                    f,
                    "{} returned an unreadable response",
                    error_name(provider).unwrap_or_default()
                )
            }
            Self::Parse { .. } => write!(f, "AI usage provider returned an unreadable response"),
            Self::Payload { provider, detail } => write!(
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

/// The short provider name the shipped error texts use. It is deliberately not
/// the report's display name: the shipped code writes "Copilot request failed
/// with HTTP 500" while the display name is "GitHub Copilot". Providers without
/// an entry use the shared, nameless wording.
fn error_name(provider: &str) -> Option<&'static str> {
    match provider {
        "claude" => Some("Claude"),
        "copilot" => Some("Copilot"),
        "cursor" => Some("Cursor"),
        "kimi" => Some("Kimi"),
        "opencode" => Some("OpenCode"),
        "antigravity" => Some("Antigravity"),
        _ => None,
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
        .and_then(|value| value.trim().parse::<f64>().ok())
        // The shipped text truncates to whole seconds (`int(seconds)`).
        .map(|seconds| {
            #[allow(clippy::cast_possible_truncation)]
            let whole = seconds as i64;
            format!("; retry in {whole}s")
        })
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
        _ => match error_name(provider) {
            Some(_) if provider == "opencode" => format!("OpenCode API error: HTTP {status}"),
            Some(name) => format!("{name} request failed with HTTP {status}"),
            None => format!("AI usage request failed with HTTP {status}"),
        },
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
