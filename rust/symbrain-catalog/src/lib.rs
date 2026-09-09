//! Deterministic, namespaced, policy-filtered MCP tool catalog.

#![deny(unsafe_code)]

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use symbrain_policy::{Report, ToolAnnotations, Verdict};

/// MCP tool definition received from one server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    /// Original tool name supplied by the server.
    pub name: String,
    /// Human-readable tool description.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Raw JSON Schema, preserved byte-for-byte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Box<RawValue>>,
    /// MCP behavioral annotations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
}

impl Tool {
    /// Creates a tool with no description, schema, or annotations.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            input_schema: None,
            annotations: None,
        }
    }
}

/// One merged catalog entry with routing and policy metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// Namespaced tool definition exposed to the harness.
    #[serde(flatten)]
    pub tool: Tool,
    /// Source server alias.
    pub server: String,
    /// Original unprefixed tool name used when routing calls.
    pub original_name: String,
    /// Exposure verdict from the policy engine.
    pub verdict: Verdict,
    /// Foreign-server read/write class.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub access_class: String,
    /// Source used to derive the foreign-server access class.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub access_source: String,
}

/// One server's live tools paired with its resolved policy report.
#[derive(Debug, Clone)]
pub struct ServerTools {
    /// Server alias used for namespacing and routing.
    pub server: String,
    /// Live MCP tool catalog for the server.
    pub tools: Vec<Tool>,
    /// Resolved policy report for the same original tool names.
    pub report: Report,
}

/// Immutable merged catalog, sorted by exposed tool name.
#[derive(Debug, Clone)]
pub struct Catalog {
    entries: Vec<Entry>,
    by_name: BTreeMap<String, usize>,
}

/// Hard startup error raised when namespacing produces a duplicate tool name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollisionError {
    /// Colliding namespaced tool name.
    pub name: String,
    /// Server that attempted to register the duplicate.
    pub server: String,
    /// Server that registered the name first.
    pub existing: String,
}

impl fmt::Display for CollisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "catalog: tool name {:?} collision: already registered by server {:?}, cannot add from {:?}",
            self.name, self.existing, self.server
        )
    }
}

impl Error for CollisionError {}

impl Catalog {
    /// Builds a deterministic catalog and rejects all post-namespace collisions.
    ///
    /// # Errors
    ///
    /// Returns [`CollisionError`] when two tools map to the same exposed name.
    pub fn build(servers: &[ServerTools]) -> Result<Self, CollisionError> {
        let mut entries = Vec::new();
        let mut seen = BTreeMap::<String, String>::new();

        for server_tools in servers {
            let prefix = server_prefix(&server_tools.server);
            for tool in &server_tools.tools {
                let name = namespace(&tool.name, prefix);
                if let Some(existing) = seen.get(&name) {
                    return Err(CollisionError {
                        name,
                        server: server_tools.server.clone(),
                        existing: existing.clone(),
                    });
                }
                seen.insert(name.clone(), server_tools.server.clone());

                let exposure = server_tools
                    .report
                    .exposures
                    .as_ref()
                    .and_then(|exposures| exposures.get(&tool.name));
                entries.push(Entry {
                    tool: Tool {
                        name,
                        description: tool.description.clone(),
                        input_schema: tool.input_schema.clone(),
                        annotations: tool.annotations.clone(),
                    },
                    server: server_tools.server.clone(),
                    original_name: tool.name.clone(),
                    verdict: server_tools.report.verdict(&tool.name),
                    access_class: exposure.map_or_else(String::new, |value| value.class.clone()),
                    access_source: exposure.map_or_else(String::new, |value| value.source.clone()),
                });
            }
        }

        entries.sort_by(|left, right| left.tool.name.cmp(&right.tool.name));
        let by_name = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.tool.name.clone(), index))
            .collect();
        Ok(Self { entries, by_name })
    }

    /// Returns exposed entries in stable name order.
    pub fn exposed(&self) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(|entry| entry.verdict == Verdict::Exposed)
    }

    /// Returns all entries in stable name order.
    #[must_use]
    pub fn all(&self) -> &[Entry] {
        &self.entries
    }

    /// Looks up an entry by its exposed namespaced name.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&Entry> {
        self.by_name.get(name).map(|index| &self.entries[*index])
    }

    /// Returns exposed tool names in stable order.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.exposed()
            .map(|entry| entry.tool.name.as_str())
            .collect()
    }
}

fn server_prefix(server: &str) -> &'static str {
    if server == "vault" { "vault_" } else { "" }
}

fn namespace(name: &str, prefix: &str) -> String {
    if prefix.is_empty() {
        return name.to_string();
    }
    for known in ["vault_", "memory_", "entity_", "graph_"] {
        if name.len() > known.len() && name.starts_with(known) {
            return name.to_string();
        }
    }
    format!("{prefix}{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(exposed: &[&str], hidden: &[&str], unknown: &[&str]) -> Report {
        Report {
            server: "vault".to_string(),
            enabled: true,
            mode: "full".to_string(),
            exposed: exposed.iter().map(ToString::to_string).collect(),
            hidden: hidden.iter().map(ToString::to_string).collect(),
            unknown: unknown.iter().map(ToString::to_string).collect(),
            exposures: None,
        }
    }

    #[test]
    fn namespace_matches_go_edge_cases() {
        for (name, prefix, expected) in [
            ("vault_get_entry", "vault_", "vault_get_entry"),
            ("memory_search", "vault_", "memory_search"),
            ("entity_list", "vault_", "entity_list"),
            ("graph_neighbors", "vault_", "graph_neighbors"),
            ("get_entry", "vault_", "vault_get_entry"),
            ("vault_", "vault_", "vault_vault_"),
            ("search", "", "search"),
        ] {
            assert_eq!(namespace(name, prefix), expected);
        }
    }

    #[test]
    fn collision_preserves_first_owner_and_exact_message() {
        let servers = vec![
            ServerTools {
                server: "vault".to_string(),
                tools: vec![Tool::new("vault_shared")],
                report: report(&["vault_shared"], &[], &[]),
            },
            ServerTools {
                server: "memory".to_string(),
                tools: vec![Tool::new("vault_shared")],
                report: report(&["vault_shared"], &[], &[]),
            },
        ];
        let error = Catalog::build(&servers).expect_err("collision must fail");
        assert_eq!(error.name, "vault_shared");
        assert_eq!(error.existing, "vault");
        assert_eq!(error.server, "memory");
        assert_eq!(
            error.to_string(),
            "catalog: tool name \"vault_shared\" collision: already registered by server \"vault\", cannot add from \"memory\""
        );
    }

    #[test]
    fn exposed_lookup_and_names_are_stable() {
        let servers = vec![ServerTools {
            server: "vault".to_string(),
            tools: vec![Tool::new("zebra"), Tool::new("alpha"), Tool::new("mystery")],
            report: report(&["zebra", "alpha"], &[], &["mystery"]),
        }];
        let catalog = Catalog::build(&servers).expect("catalog builds");
        assert_eq!(catalog.names(), ["vault_alpha", "vault_zebra"]);
        assert_eq!(catalog.all().len(), 3);
        assert_eq!(
            catalog.lookup("vault_mystery").expect("lookup").verdict,
            Verdict::Unknown
        );
    }

    #[test]
    fn input_schema_bytes_survive_catalog_construction() {
        let raw = r#"{ "properties" : {"id":{"type":"string"}}, "type" : "object" }"#;
        let server = ServerTools {
            server: "vault".to_string(),
            tools: vec![Tool {
                name: "get_entry".to_string(),
                description: String::new(),
                input_schema: Some(RawValue::from_string(raw.to_string()).expect("valid schema")),
                annotations: None,
            }],
            report: report(&["get_entry"], &[], &[]),
        };
        let catalog = Catalog::build(&[server]).expect("catalog builds");
        let schema = catalog
            .lookup("vault_get_entry")
            .and_then(|entry| entry.tool.input_schema.as_deref())
            .expect("schema exists");
        assert_eq!(schema.get(), raw);
    }

    #[test]
    fn foreign_exposure_metadata_is_carried() {
        let mut exposures = BTreeMap::new();
        exposures.insert(
            "search".to_string(),
            symbrain_policy::ToolExposure {
                class: "read".to_string(),
                source: "read_only_hint".to_string(),
            },
        );
        let server = ServerTools {
            server: "zotero".to_string(),
            tools: vec![Tool::new("search")],
            report: Report {
                server: "zotero".to_string(),
                enabled: true,
                mode: String::new(),
                exposed: vec!["search".to_string()],
                hidden: Vec::new(),
                unknown: Vec::new(),
                exposures: Some(exposures),
            },
        };
        let catalog = Catalog::build(&[server]).expect("catalog builds");
        let entry = catalog.lookup("search").expect("lookup");
        assert_eq!(entry.access_class, "read");
        assert_eq!(entry.access_source, "read_only_hint");
    }
}
