#[path = "skills_catalog.rs"]
mod skills_catalog;
use crate::Gateway;
use crate::response::{ListedTool, ToolAnnotations};
use symbrain_policy::Profile;

pub(crate) const MEMORY_TOOLS: &[&str] = &[
    "memory_get",
    "memory_set",
    "memory_search",
    "memory_list",
    "entity_list",
    "entity_relate",
    "entity_resolve",
    "graph_neighbors",
    "memory_candidates",
    "memory_promote",
    "memory_reject",
    "query_log",
];
pub(crate) const ACTIVITY_TOOLS: &[&str] = &["activity_search", "activity_get", "activity_status"];
pub(crate) use symbrain_mcp::SKILLS_TOOL_NAMES as SKILLS_TOOLS;

fn schema(name: &str) -> Box<serde_json::value::RawValue> {
    let text = match name {
        "memory_promote" | "memory_reject" => {
            r#"{"type":"object","properties":{"id":{"type":"string"},"client_id":{"type":"string"},"with_evidence":{"type":"boolean"}},"required":["id"]}"#
        }
        "memory_candidates" | "query_log" => {
            r#"{"type":"object","properties":{"scope":{"type":"string"},"limit":{"type":"integer"},"max_sensitivity":{"type":"string"},"min_sharing_level":{"type":"string"},"client_id":{"type":"string"},"as_of":{"type":"string"},"max_payload_bytes":{"type":"integer"},"cursor":{"type":"string"}}}"#
        }
        "skills_list" => r#"{"type":"object","properties":{}}"#,
        "skills_inspect" | "skills_validate" => {
            r#"{"type":"object","properties":{"path":{"type":"string"},"name":{"type":"string"}}}"#
        }
        "skills_targets_status" => r#"{"type":"object","properties":{"scope":{"type":"string"}}}"#,
        "skills_render_plan" => {
            r#"{"type":"object","properties":{"path":{"type":"string"},"name":{"type":"string"},"target":{"type":"string"},"profile":{"type":"string"},"dry_run":{"type":"boolean"}}}"#
        }
        "skills_install" => {
            r#"{"type":"object","properties":{"path":{"type":"string"},"name":{"type":"string"},"target":{"type":"string"},"profile":{"type":"string"},"dry_run":{"type":"boolean"},"mode":{"type":"string"}}}"#
        }
        _ => r#"{"type":"object"}"#,
    };
    serde_json::value::RawValue::from_string(text.to_string()).expect("static schema")
}

fn listed(name: &str) -> ListedTool {
    if SKILLS_TOOLS.contains(&name) {
        return skills_catalog::listed(name);
    }
    if let Some(tool) = super::memory_catalog::listed(name) {
        return tool;
    }
    let read = !matches!(
        name,
        "memory_set"
            | "memory_promote"
            | "memory_reject"
            | "entity_relate"
            | "skills_render_plan"
            | "skills_install"
    );
    ListedTool {
        annotations: ToolAnnotations {
            title: name.replace('_', " "),
            read_only_hint: read,
            destructive_hint: matches!(
                name,
                "memory_reject" | "entity_relate" | "skills_render_plan" | "skills_install"
            ),
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
        let mut tools = Vec::new();
        if self.memory.is_some() {
            tools.extend(
                MEMORY_TOOLS
                    .iter()
                    .filter(|name| {
                        self.memory_tool_names
                            .iter()
                            .any(|allowed| allowed == **name)
                    })
                    .chain(ACTIVITY_TOOLS.iter().filter(|name| {
                        self.activity_tool_names
                            .iter()
                            .any(|allowed| allowed == **name)
                    }))
                    .map(|name| listed(name)),
            );
        }
        if self.skills_allowed {
            tools.extend(self.skills_tool_names.iter().map(|name| listed(name)));
        }
        tools
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

pub(crate) fn exposed_skills(profile: &Profile) -> Vec<String> {
    if profile.server("skills").enabled {
        SKILLS_TOOLS.iter().map(|&s| s.to_string()).collect()
    } else {
        Vec::new()
    }
}
