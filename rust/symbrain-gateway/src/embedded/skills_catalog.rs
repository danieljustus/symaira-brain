//! Exact Skills tool registration metadata from the immutable Go bodies.
use crate::response::{ListedTool, ToolAnnotations};

pub(super) fn listed(name: &str) -> ListedTool {
    let (description, schema, title, read, destructive) = match name {
        "skills_list" => (
            "List skills in the symskills library.",
            r#"{"type":"object","properties":{}}"#,
            "List Skills",
            true,
            false,
        ),
        "skills_inspect" => (
            "Inspect one skill by path or library name.",
            r#"{"type":"object","properties":{"path":{"type":"string"},"name":{"type":"string"}}}"#,
            "Inspect Skill",
            true,
            false,
        ),
        "skills_validate" => (
            "Validate one skill by path or library name.",
            r#"{"type":"object","properties":{"path":{"type":"string"},"name":{"type":"string"}}}"#,
            "Validate Skill",
            true,
            false,
        ),
        "skills_profile_list" => (
            "List available context profiles (global and project).",
            r#"{"type":"object","properties":{}}"#,
            "List Skill Profiles",
            true,
            false,
        ),
        "skills_profile_resolve" => (
            "Resolve a context profile and return the merged skill set.",
            r#"{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}"#,
            "Resolve Skill Profile",
            true,
            false,
        ),
        "skills_render_plan" => (
            "Render a skill or profile to the managed artifact directory and return planned target paths. Pass dry_run=true to preview without writing.",
            r#"{"type":"object","properties":{"path":{"type":"string"},"name":{"type":"string"},"target":{"type":"string"},"profile":{"type":"string"},"dry_run":{"type":"boolean"}}}"#,
            "Render Skill Plan",
            false,
            true,
        ),
        "skills_install" => (
            "Render and install a skill or profile. Dry-run defaults to true; pass dry_run=false for writes.",
            r#"{"type":"object","properties":{"path":{"type":"string"},"name":{"type":"string"},"target":{"type":"string"},"scope":{"type":"string"},"dry_run":{"type":"boolean"},"profile":{"type":"string"}}}"#,
            "Install Skill",
            false,
            true,
        ),
        "skills_targets_status" => (
            "Read-only inventory and readiness status for supported AI-agent harnesses.",
            r#"{"type":"object","properties":{"scope":{"type":"string"}}}"#,
            "Skill Target Status",
            true,
            false,
        ),
        "skills_discover_sources" => (
            "Discover unmanaged skill sources in known harness roots or explicit paths.",
            r#"{"type":"object","properties":{"paths":{"type":"array","items":{"type":"string"}},"scope":{"type":"string"}}}"#,
            "Discover Skill Sources",
            true,
            false,
        ),
        "skills_history" => (
            "List the versioned commit history of a library skill: revision, timestamp, operation (import/update/restore/unknown) and changed files.",
            r#"{"type":"object","properties":{"name":{"type":"string"},"limit":{"type":"integer"}},"required":["name"]}"#,
            "Skill History",
            true,
            false,
        ),
        "skills_restore" => (
            "Roll a library skill's files back to a previous revision by forward commit. Dry-run defaults to true; pass dry_run=false to write. Refuses invalid restored states and never discards uncommitted changes (pass allow_dirty=true to snapshot them first); sync=true re-installs stale targets.",
            r#"{"type":"object","properties":{"name":{"type":"string"},"rev":{"type":"string"},"dry_run":{"type":"boolean"},"allow_dirty":{"type":"boolean"},"sync":{"type":"boolean"}},"required":["name","rev"]}"#,
            "Restore Skill",
            false,
            true,
        ),
        _ => unreachable!("registered skills tool"),
    };
    ListedTool {
        name: name.into(),
        description: description.into(),
        input_schema: Some(
            serde_json::value::RawValue::from_string(schema.into()).expect("static skills schema"),
        ),
        annotations: ToolAnnotations {
            title: title.into(),
            read_only_hint: read,
            destructive_hint: destructive,
            idempotent_hint: false,
            open_world_hint: false,
        },
    }
}
