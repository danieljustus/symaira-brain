use super::{Gateway, GatewayError};
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
struct BootstrapResponse {
    profile: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    profile_description: String,
    generated_at: String,
    servers: Vec<BootstrapServer>,
    catalog: Vec<String>,
    vault: BootstrapVault,
}

#[derive(Serialize)]
struct BootstrapServer {
    server: String,
    enabled: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    mode: String,
    exposed_tools: Option<Vec<String>>,
    exposed_count: usize,
}

#[derive(Serialize)]
struct BootstrapVault {
    status: String,
    listing: String,
}

#[derive(Serialize)]
struct PatternsResponse {
    profile: String,
    threshold: usize,
    patterns: Option<Vec<symbrain_patterns::Pattern>>,
}

impl Gateway {
    pub(super) fn instructions(&self) -> String {
        if self.degradations.is_empty() {
            return format!("symbrain profile {:?}", self.profile.name);
        }
        let servers = self
            .degradations
            .iter()
            .map(|degradation| degradation.server.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "symbrain profile {:?}; degraded backends: {servers}",
            self.profile.name
        )
    }

    pub(super) fn bootstrap(&self) -> Result<String, GatewayError> {
        let mut per_server = BTreeMap::<String, Vec<String>>::new();
        for entry in self.catalog.exposed() {
            per_server
                .entry(entry.server.clone())
                .or_default()
                .push(entry.tool.name.clone());
        }
        let servers = ["vault", "memory", "skills"]
            .into_iter()
            .map(|alias| {
                let config = self.profile.server(alias);
                let tools = per_server.remove(alias);
                let exposed_count = tools.as_ref().map_or(0, Vec::len);
                BootstrapServer {
                    server: alias.to_string(),
                    enabled: config.enabled,
                    mode: config.mode,
                    exposed_tools: tools,
                    exposed_count,
                }
            })
            .collect::<Vec<_>>();
        let vault = self.profile.server("vault");
        let vault_status = if !vault.enabled || vault.mode == symbrain_policy::VAULT_MODE_OFF {
            "disabled"
        } else if self.servers.contains_key("vault") {
            "present"
        } else {
            "absent"
        };
        serde_json::to_string(&BootstrapResponse {
            profile: self.profile.name.clone(),
            profile_description: self.profile.description.clone(),
            generated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            servers,
            catalog: self
                .catalog
                .names()
                .into_iter()
                .map(str::to_string)
                .collect(),
            vault: BootstrapVault {
                status: vault_status.to_string(),
                listing: "unavailable-without-unlock".to_string(),
            },
        })
        .map_err(|error| GatewayError::Serialization(error.to_string()))
    }

    pub(super) fn patterns(&self) -> Result<String, GatewayError> {
        let directory = symbrain_core::xdg::patterns_dir()
            .ok_or_else(|| GatewayError::Policy("patterns: resolve data directory".to_string()))?;
        let path = directory.join(format!("{}.jsonl", self.profile.name));
        let episodes = symbrain_patterns::Store::new_private(path)
            .load()
            .map_err(|error| GatewayError::Policy(format!("patterns: {error}")))?;
        let patterns = symbrain_patterns::promote(&episodes, 3);
        serde_json::to_string(&PatternsResponse {
            profile: self.profile.name.clone(),
            threshold: 3,
            patterns: (!patterns.is_empty()).then_some(patterns),
        })
        .map_err(|error| GatewayError::Serialization(error.to_string()))
    }
}
