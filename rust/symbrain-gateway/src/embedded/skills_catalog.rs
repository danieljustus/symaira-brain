//! Registration metadata of the profile and versioning Skills tools, taken
//! verbatim from the Go registrations (schemas in Go's wire key order).

use crate::response::{ListedTool, ToolAnnotations};

/// Tools implemented by [`super::skills_tools`], in Go registration order.
pub(super) const TOOLS: &[&str] = &[
    "skills_profile_list",
    "skills_profile_resolve",
    "skills_discover_sources",
    "skills_history",
    "skills_restore",
];

pub(super) fn listed(name: &str) -> Option<ListedTool> {
    let (description, schema, title, read_only, destructive) = match name {
        "skills_profile_list" => (
            "List available context profiles (global and project).",
            r#"{"properties":{},"type":"object"}"#,
            "List Skill Profiles",
            true,
            false,
        ),
        "skills_profile_resolve" => (
            "Resolve a context profile and return the merged skill set.",
            r#"{"properties":{"name":{"type":"string"}},"required":["name"],"type":"object"}"#,
            "Resolve Skill Profile",
            true,
            false,
        ),
        "skills_discover_sources" => (
            "Discover unmanaged skill sources in known harness roots or explicit paths.",
            r#"{"properties":{"paths":{"items":{"type":"string"},"type":"array"},"scope":{"type":"string"}},"type":"object"}"#,
            "Discover Skill Sources",
            true,
            false,
        ),
        "skills_history" => (
            "List the versioned commit history of a library skill: revision, timestamp, operation (import/update/restore/unknown) and changed files.",
            r#"{"properties":{"limit":{"type":"integer"},"name":{"type":"string"}},"required":["name"],"type":"object"}"#,
            "Skill History",
            true,
            false,
        ),
        "skills_restore" => (
            "Roll a library skill's files back to a previous revision by forward commit. Dry-run defaults to true; pass dry_run=false to write. Refuses invalid restored states and never discards uncommitted changes (pass allow_dirty=true to snapshot them first); sync=true re-installs stale targets.",
            r#"{"properties":{"allow_dirty":{"type":"boolean"},"dry_run":{"type":"boolean"},"name":{"type":"string"},"rev":{"type":"string"},"sync":{"type":"boolean"}},"required":["name","rev"],"type":"object"}"#,
            "Restore Skill",
            false,
            true,
        ),
        _ => return None,
    };
    Some(ListedTool {
        name: name.into(),
        description: description.into(),
        input_schema: Some(
            serde_json::value::RawValue::from_string(schema.into()).expect("static skills schema"),
        ),
        annotations: ToolAnnotations {
            title: title.into(),
            read_only_hint: read_only,
            destructive_hint: destructive,
            idempotent_hint: false,
            open_world_hint: false,
        },
    })
}
