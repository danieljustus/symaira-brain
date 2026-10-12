//! Go skills tool payloads; internal capability handles never enter JSON.
use crate::{Bundle, Frontmatter, Rendered};
use serde_json::{Value, json};

/// Frontmatter omits optional empty fields, matching the shipped Go tags.
/// # Panics
/// Panics if the typed frontmatter cannot serialize to its required JSON object.
#[must_use]
pub fn frontmatter(value: &Frontmatter) -> Value {
    let mut value = serde_json::to_value(value).expect("frontmatter is serializable");
    value
        .as_object_mut()
        .expect("frontmatter object")
        .retain(|key, value| {
            matches!(key.as_str(), "name" | "description")
                || !(value.is_null()
                    || value.as_str() == Some("")
                    || value.as_array().is_some_and(Vec::is_empty)
                    || value.as_object().is_some_and(serde_json::Map::is_empty))
        });
    value
}
/// Full inspection shape without root capabilities and internal read buffers.
/// # Panics
/// Panics if the typed manifest cannot serialize to its required JSON object and skill field.
#[must_use]
pub fn bundle(value: &Bundle) -> Value {
    let mut manifest = serde_json::to_value(&value.manifest).expect("manifest serializable");
    let fields = manifest.as_object_mut().expect("manifest object");
    if value.manifest.terms.is_empty() {
        fields.remove("terms");
    }
    if value.manifest.skill.requires.is_empty() {
        fields
            .get_mut("skill")
            .expect("skill field")
            .as_object_mut()
            .expect("skill object")
            .remove("requires");
    }
    json!({"root":crate::GoText::from_path(&value.root),"frontmatter":frontmatter(&value.frontmatter),"manifest":manifest,"body":value.body,"resources":value.resources})
}
/// Render summary exposes generated Markdown as text and omits private files.
/// # Panics
/// Panics if a typed render or variants value cannot serialize to its required JSON object.
#[must_use]
pub fn rendered(value: &Rendered, path: &std::path::Path) -> Value {
    let mut result = json!({"target":value.target,"name":value.name,"path":crate::GoText::from_path(path),"frontmatter":frontmatter(&value.frontmatter),"skill_md":crate::GoText::from_bytes(&value.skill_md)});
    let map = result.as_object_mut().expect("render object");
    if !value.source.is_empty() {
        map.insert("source".into(), json!(value.source));
    }
    if !value.profile.is_empty() {
        map.insert("profile".into(), json!(value.profile));
    }
    if !value.warnings.is_empty() {
        map.insert("warnings".into(), json!(value.warnings));
    }
    if !value.unmet_requirements.is_empty() {
        map.insert("unmet_requirements".into(), json!(value.unmet_requirements));
    }
    if let Some(variants) = &value.variants {
        let mut variants = serde_json::to_value(variants).expect("variants serializable");
        variants
            .as_object_mut()
            .expect("variants object")
            .retain(|key, val| {
                matches!(key.as_str(), "replaced_bytes" | "source_bytes")
                    || !(val.as_array().is_some_and(Vec::is_empty)
                        || val.as_object().is_some_and(serde_json::Map::is_empty))
            });
        map.insert("variants".into(), variants);
    }
    result
}

/// Decodes a Go JSON string, replacing each invalid byte and lone surrogate.
/// # Errors
/// Returns the JSON type/syntax error after Unicode repair.
pub fn decode_string(raw: &[u8]) -> Result<String, serde_json::Error> {
    serde_json::from_str(&crate::install::marker_string::repair_json_strings(raw))
}

/// Repairs Go argument string values and keys without decoding numeric fields.
/// Duplicate fields, valid escapes and raw non-string tokens are retained.
#[must_use]
pub fn repair_argument_strings(raw: &[u8]) -> String {
    crate::install::marker_string::repair_json_strings(raw)
}
