//! Versioned metadata of the existing profile-reachable Memory/Activity tools.

use crate::response::{ListedTool, ToolAnnotations};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct Descriptor {
    annotations: Annotations,
    description: String,
    #[serde(rename = "inputSchema")]
    input_schema: Box<RawValue>,
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Annotations {
    title: String,
    #[serde(default)]
    read_only_hint: bool,
    #[serde(default)]
    destructive_hint: bool,
    #[serde(default)]
    idempotent_hint: bool,
    #[serde(default)]
    open_world_hint: bool,
}

pub(super) fn listed(name: &str) -> Option<ListedTool> {
    static DESCRIPTORS: OnceLock<Vec<Descriptor>> = OnceLock::new();
    let descriptors = DESCRIPTORS.get_or_init(|| {
        serde_json::from_str(include_str!("memory_catalog.json"))
            .expect("versioned Memory catalog is valid JSON")
    });
    let entry = descriptors.iter().find(|entry| entry.name == name)?;
    Some(ListedTool {
        annotations: ToolAnnotations {
            title: entry.annotations.title.clone(),
            read_only_hint: entry.annotations.read_only_hint,
            destructive_hint: entry.annotations.destructive_hint,
            idempotent_hint: entry.annotations.idempotent_hint,
            open_world_hint: entry.annotations.open_world_hint,
        },
        description: entry.description.clone(),
        input_schema: Some(entry.input_schema.clone()),
        name: entry.name.clone(),
    })
}
