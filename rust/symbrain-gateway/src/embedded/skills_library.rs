//! Read-only library, profile and target MCP contracts.
use super::{Error, bundle, compact, text};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use symbrain_skills::{config, context_profile, metadata, targets_status};

#[derive(Serialize)]
struct Item {
    name: String,
    description: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    category: String,
    root: String,
    #[serde(flatten)]
    record: metadata::Record,
}
fn metadata_options() -> metadata::Options {
    // Gateway Register passes no HomeDir or EventsPath. No logger or marker
    // fallback is fabricated; explicit library timestamps still come from FS.
    metadata::Options::default()
}
pub(super) fn list() -> Result<String, Error> {
    let (entries, issues) = symbrain_skills::library::list_library(&config::defaults().library_dir);
    let mut counts = BTreeMap::new();
    let items: Vec<_> = entries
        .into_iter()
        .map(|entry| {
            if !entry.category.is_empty() {
                *counts.entry(entry.category.clone()).or_insert(0_usize) += 1;
            }
            let record = metadata::collect(
                std::path::Path::new(&entry.path),
                &entry.name,
                &metadata_options(),
            );
            Item {
                name: entry.name,
                description: entry.description,
                category: entry.category,
                root: entry.path,
                record,
            }
        })
        .collect();
    // Go list returns a nil issues slice as null; CLI alone normalizes it to [].
    compact(
        &json!({"skills":items,"category_counts":counts,"issues":if issues.is_empty() { Value::Null } else { json!(issues) }}),
    )
}
pub(super) fn inspect(value: &Value) -> Result<String, Error> {
    let bundle = bundle(value)?;
    let mut wire = symbrain_skills::wire::bundle(&bundle);
    let record = metadata::collect(&bundle.root, &bundle.frontmatter.name, &metadata_options());
    let fields =
        serde_json::to_value(record).map_err(|error| Error::internal("serialize result", error))?;
    wire.as_object_mut()
        .expect("bundle object")
        .extend(fields.as_object().expect("metadata object").clone());
    compact(&wire)
}
pub(super) fn validate(value: &Value) -> Result<String, Error> {
    let bundle = bundle(value)?;
    let issues = symbrain_skills::validate(&bundle);
    compact(
        &json!({"valid":!issues.iter().any(|issue| issue.severity=="error"),"issues":if issues.is_empty() { Value::Null } else { json!(issues) }}),
    )
}
pub(super) fn profiles() -> Result<String, Error> {
    let refs = context_profile::list(&config::defaults().profiles_dir, None)
        .map_err(|error| Error::validation("list profiles", error))?;
    compact(&json!({"profiles":if refs.is_empty() { Value::Null } else { json!(refs) }}))
}
pub(super) fn resolve_profile(value: &Value) -> Result<String, Error> {
    let config = config::defaults();
    let (rows, issues) = context_profile::resolve(
        &config.library_dir,
        &config.profiles_dir,
        None,
        &text(value, "name")?,
    )
    .map_err(|error| Error::validation("resolve profile", error))?;
    compact(
        &json!({"skills":if rows.is_empty() {Value::Null} else {json!(rows)},"issues":if issues.is_empty() {Value::Null} else {json!(issues)}}),
    )
}
pub(super) fn targets(value: &Value) -> Result<String, Error> {
    let _scope = text(value, "scope")?;
    let rows = targets_status::list_status(&targets_status::StatusOptions {
        home_dir: config::home_dir(),
        project_dir: None,
        scope: "user".into(),
    });
    compact(&json!({"targets":rows}))
}
