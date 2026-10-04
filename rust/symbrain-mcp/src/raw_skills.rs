//! Only Skills anonymous argument structs retain Go RawMessage admission.
use serde_json::value::RawValue;
use std::collections::BTreeMap;

/// Original CoreKit registration order; also scopes raw transport admission.
pub const SKILLS_TOOL_NAMES: &[&str] = &[
    "skills_list",
    "skills_inspect",
    "skills_validate",
    "skills_profile_list",
    "skills_profile_resolve",
    "skills_render_plan",
    "skills_install",
    "skills_discover_sources",
    "skills_history",
    "skills_restore",
    "skills_targets_status",
];

/// Reads a known Skills call's outer fields without decoding argument values.
/// Other tool calls retain their existing Value admission and dispatch.
#[must_use]
pub fn raw_skills_params(params: Option<&RawValue>) -> Option<(String, Option<Box<RawValue>>)> {
    let mut fields: BTreeMap<String, Box<RawValue>> = serde_json::from_str(params?.get()).ok()?;
    let name: String = serde_json::from_str(fields.remove("name")?.get()).ok()?;
    SKILLS_TOOL_NAMES
        .contains(&name.as_str())
        .then(|| (name, fields.remove("arguments")))
}
