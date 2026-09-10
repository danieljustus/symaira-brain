use crate::Gateway;
use crate::response::{ListedTool, ToolAnnotations};
use symbrain_policy::Profile;

pub(crate) const MEMORY_TOOLS: &[&str] = &[
    "entity_list",
    "entity_relate",
    "entity_resolve",
    "graph_neighbors",
    "memory_candidates",
    "memory_get",
    "memory_list",
    "memory_promote",
    "memory_reject",
    "memory_search",
    "memory_set",
    "query_log",
];
pub(crate) const ACTIVITY_TOOLS: &[&str] = &["activity_get", "activity_search", "activity_status"];

fn schema(name: &str) -> Box<serde_json::value::RawValue> {
    let text = match name {
        "memory_get" | "memory_promote" | "memory_reject" => {
            r#"{"type":"object","properties":{"id":{"type":"string"},"client_id":{"type":"string"},"with_evidence":{"type":"boolean"}},"required":["id"]}"#
        }
        "memory_set" => {
            r#"{"type":"object","properties":{"content":{"type":"string"},"kind":{"type":"string"},"scope":{"type":"string"},"metadata":{"type":"string"},"session_id":{"type":"string"},"entities":{"type":"string"},"working":{"type":"boolean"},"staged":{"type":"boolean"}},"required":["content","kind"]}"#
        }
        "memory_search" => {
            r#"{"type":"object","properties":{"query":{"type":"string"},"scope":{"type":"string"},"session_id":{"type":"string"},"profile":{"type":"string"},"limit":{"type":"integer"},"entity":{"type":"string"},"min_confidence":{"type":"string"},"verification":{"type":"string"},"exclude_superseded":{"type":"boolean"},"max_age":{"type":"string"},"max_sensitivity":{"type":"string"},"min_sharing_level":{"type":"string"},"client_id":{"type":"string"},"with_evidence":{"type":"boolean"},"min_score":{"type":"number"},"max_payload_bytes":{"type":"integer"},"cursor":{"type":"string"}},"required":["query"]}"#
        }
        "memory_list" | "memory_candidates" | "query_log" => {
            r#"{"type":"object","properties":{"scope":{"type":"string"},"limit":{"type":"integer"},"max_sensitivity":{"type":"string"},"min_sharing_level":{"type":"string"},"client_id":{"type":"string"},"as_of":{"type":"string"},"max_payload_bytes":{"type":"integer"},"cursor":{"type":"string"}}}"#
        }
        "activity_search" => {
            r#"{"type":"object","properties":{"query":{"type":"string"},"source":{"type":"string"},"from":{"type":"string"},"to":{"type":"string"},"limit":{"type":"integer"},"max_tokens":{"type":"integer"},"include_episodes":{"type":"boolean"}},"required":["query","from","to","limit","max_tokens"]}"#
        }
        "activity_get" => {
            r#"{"type":"object","properties":{"id":{"type":"string"},"max_tokens":{"type":"integer"}},"required":["id","max_tokens"]}"#
        }
        "activity_status" => {
            r#"{"type":"object","properties":{"max_tokens":{"type":"integer"}},"required":["max_tokens"]}"#
        }
        "entity_resolve" => {
            r#"{"type":"object","properties":{"query":{"type":"string"},"type":{"type":"string"},"aliases":{"type":"string"},"limit":{"type":"integer"}},"required":["query"]}"#
        }
        "entity_relate" => {
            r#"{"type":"object","properties":{"from":{"type":"string"},"to":{"type":"string"},"from_id":{"type":"string"},"to_id":{"type":"string"},"relation":{"type":"string"},"action":{"type":"string"},"source":{"type":"string"},"source_ref":{"type":"string"},"verification":{"type":"string"},"evidence":{"type":"string"},"valid_from":{"type":"string"},"valid_until":{"type":"string"}},"required":["relation"]}"#
        }
        "graph_neighbors" => {
            r#"{"type":"object","properties":{"entity":{"type":"string"},"depth":{"type":"integer"}},"required":["entity"]}"#
        }
        _ => r#"{"type":"object"}"#,
    };
    serde_json::value::RawValue::from_string(text.to_string()).expect("static schema")
}

fn listed(name: &str) -> ListedTool {
    let read = !matches!(
        name,
        "memory_set" | "memory_promote" | "memory_reject" | "entity_relate"
    );
    ListedTool {
        annotations: ToolAnnotations {
            title: name.replace('_', " "),
            read_only_hint: read,
            destructive_hint: matches!(name, "memory_reject" | "entity_relate"),
            idempotent_hint: read,
            open_world_hint: false,
        },
        description: format!("Native embedded {name} tool."),
        input_schema: Some(schema(name)),
        name: name.into(),
    }
}

impl Gateway {
    pub(crate) fn embedded_tools(&self) -> Vec<ListedTool> {
        if self.memory.is_none() {
            return Vec::new();
        }
        self.memory_tool_names
            .iter()
            .chain(self.activity_tool_names.iter())
            .map(|name| listed(name))
            .collect()
    }
}

pub(crate) fn exposed_native(profile: &Profile) -> (Vec<String>, Vec<String>) {
    let cfg = profile.server(symbrain_policy::SERVER_MEMORY);
    let Ok(report) = symbrain_policy::evaluate_preset(symbrain_policy::SERVER_MEMORY, &cfg) else {
        return (Vec::new(), Vec::new());
    };
    let memory = report
        .exposed
        .iter()
        .filter(|name| MEMORY_TOOLS.contains(&name.as_str()))
        .cloned()
        .collect();
    let activity = report
        .exposed
        .iter()
        .filter(|name| ACTIVITY_TOOLS.contains(&name.as_str()))
        .cloned()
        .collect();
    (memory, activity)
}
