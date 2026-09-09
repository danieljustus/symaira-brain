use serde::{Deserialize, Serialize};

/// Redacted transport metadata for a configured MCP server.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Server name in the harness map.
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    /// Explicit or inferred transport (`stdio` or `http`).
    pub transport: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    /// Stdio executable, when configured.
    pub command: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    /// Stdio arguments, when configured.
    pub args: Vec<String>,
    #[serde(rename = "url", skip_serializing_if = "String::is_empty", default)]
    /// HTTP endpoint, when configured.
    pub url: String,
    #[serde(rename = "env_names", skip_serializing_if = "Vec::is_empty", default)]
    /// Environment variable names only; values are never exposed.
    pub env_names: Vec<String>,
}

impl ServerInfo {
    pub(crate) fn from_fields(name: &str, fields: &[(String, ValueKind)]) -> Self {
        let mut info = Self {
            name: name.to_owned(),
            ..Self::default()
        };
        let mut transport = String::new();
        let mut kind = String::new();
        for (key, value) in fields {
            match (key.as_str(), value) {
                ("command", ValueKind::String(value)) => info.command.clone_from(value),
                ("url", ValueKind::String(value)) => info.url.clone_from(value),
                ("transport", ValueKind::String(value)) => transport.clone_from(value),
                ("type", ValueKind::String(value)) => kind.clone_from(value),
                ("args", ValueKind::Strings(values)) => info.args.clone_from(values),
                ("env", ValueKind::Names(values)) => info.env_names.clone_from(values),
                _ => {}
            }
        }
        info.transport = if !transport.is_empty() {
            transport
        } else if !kind.is_empty() {
            kind
        } else if !info.url.is_empty() {
            "http".into()
        } else {
            "stdio".into()
        };
        info.env_names.sort();
        info
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ValueKind {
    String(String),
    Strings(Vec<String>),
    Names(Vec<String>),
}
