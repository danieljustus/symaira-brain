//! Target-specific rendering for loaded skill bundles.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};
use toml_edit::{DocumentMut, Item};

use crate::encode::encode_skill_md;
use crate::load::read_bundle_bytes;
use crate::model::{
    Bundle, Frontmatter, MAX_INPUT_SIZE, Manifest, SkillError, TargetConfig, validate_skill_name,
};
use crate::render_variants::resolve_variants;
use crate::target;
use crate::validation::{is_render_blocking, validate_with_targets};

/// Metadata supplied by a profile-aware caller.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderMetadata {
    /// Optional source label carried on the render result.
    pub source: String,
    /// Optional profile label carried on the render result.
    pub profile: String,
    /// Profile alias, with precedence over the manifest target alias.
    pub alias: String,
    /// Explicit capability declarations for this target runtime.
    pub capabilities: BTreeMap<String, bool>,
    /// Force rendering despite explicitly unsupported capabilities.
    pub ignore_capabilities: bool,
}

/// One unmet capability requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapabilityGap {
    /// Required capability name.
    pub capability: String,
    /// `unsupported` for a declared false capability, or `unknown` when absent.
    pub state: String,
}

/// Summary of variant changes applied to a render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VariantReport {
    /// Replaced block identifiers.
    pub blocks: Vec<String>,
    /// Target-resolved terms.
    pub terms: BTreeMap<String, String>,
    /// Markdown resources whose bytes changed.
    pub files: Vec<String>,
    /// Bytes in canonical text processed by the variant engine.
    pub source_bytes: usize,
    /// Bytes supplied by replacements.
    pub replaced_bytes: usize,
}

/// Rendered target output before filesystem materialization.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rendered {
    /// Canonical target identity.
    pub target: String,
    /// Resolved install name.
    pub name: String,
    /// Resolved frontmatter.
    pub frontmatter: Frontmatter,
    /// Exact generated SKILL.md bytes.
    pub skill_md: Vec<u8>,
    /// Changed Markdown resources, keyed by slash-separated path.
    pub files: BTreeMap<String, Vec<u8>>,
    /// Non-fatal capability diagnostics.
    pub warnings: Vec<String>,
    /// Explicitly unsupported requirements when forced.
    pub unmet_requirements: Vec<CapabilityGap>,
    /// Optional provenance source.
    pub source: String,
    /// Optional provenance profile.
    pub profile: String,
    /// Variant summary, absent for a no-op render.
    pub variants: Option<VariantReport>,
}

/// Renders one registered target. The canonical harness registry governs
/// accepted names and ordering, while target specs provide capability and
/// metadata contracts.
///
/// # Errors
/// Returns an error for disabled targets, unsupported targets, invalid overlays,
/// invalid variants, or an unsafe resolved skill name.
pub fn render_target(
    bundle: &Bundle,
    target_name: &str,
    metadata: &RenderMetadata,
) -> Result<Rendered, SkillError> {
    let Some(_spec) = target::lookup(target_name) else {
        return Err(SkillError(format!("unknown target {target_name:?}")));
    };
    // Match Go's render contract: bundle validation runs before any target
    // overlay or variant is resolved, and only render-blocking findings stop
    // output. Preserve the validator's issue order and first diagnostic.
    let known_targets = target::target_names();
    for issue in validate_with_targets(bundle, &known_targets) {
        if issue.severity == "error" && is_render_blocking(&issue.code) {
            return Err(SkillError(format!("validation error: {}", issue.message)));
        }
    }
    let config = bundle.manifest.targets.get(target_name);
    if config.is_some_and(|config| !config.enabled) {
        return Err(SkillError(format!("target {target_name} is disabled")));
    }

    let (unsupported, unknown) = capability_gaps(&bundle.manifest, target_name, metadata)?;
    if !unsupported.is_empty() && !metadata.ignore_capabilities {
        return Err(SkillError(format!(
            "target {target_name} does not support {}; disable the target in symskills.toml, or render with --ignore-capabilities to produce output that does not claim compatibility",
            describe_gaps(&unsupported)
        )));
    }
    let mut warnings = Vec::new();
    if !unsupported.is_empty() {
        warnings.push(format!(
            "rendered for {target_name} despite unsupported {}; the result does not declare compatibility with this target",
            describe_gaps(&unsupported)
        ));
    }
    if !unknown.is_empty() {
        warnings.push(format!(
            "target {target_name} has not declared {} required by this skill; rendering anyway — record what your harness supports under [capabilities.{target_name}] in config.toml",
            describe_gaps(&unknown)
        ));
    }

    let mut frontmatter = bundle.frontmatter.clone();
    let config = config.cloned().unwrap_or_default();
    if !metadata.alias.is_empty() {
        frontmatter.name.clone_from(&metadata.alias);
    } else if !config.alias.is_empty() {
        frontmatter.name.clone_from(&config.alias);
    } else if !bundle.manifest.skill.name.is_empty() {
        frontmatter.name.clone_from(&bundle.manifest.skill.name);
    }
    if !config.description.is_empty() {
        frontmatter.description.clone_from(&config.description);
    }
    target_name.clone_into(&mut frontmatter.compatibility);
    filter_and_merge_metadata(&mut frontmatter, target_name, &config.metadata);
    apply_frontmatter_overlay(bundle, target_name, &mut frontmatter)?;
    if !unsupported.is_empty() {
        frontmatter.compatibility.clear();
    }
    validate_skill_name(&frontmatter.name).map_err(|error| {
        SkillError(format!(
            "invalid resolved name for target {target_name}: {error}"
        ))
    })?;

    let composed = compose_body(bundle, target_name, &config)?;
    let (body, files, variants) = resolve_variants(bundle, target_name, &composed)?;
    let skill_md = encode_skill_md(&frontmatter, &body)?;
    Ok(Rendered {
        target: target_name.to_owned(),
        name: frontmatter.name.clone(),
        frontmatter,
        skill_md,
        files,
        warnings,
        unmet_requirements: unsupported,
        source: metadata.source.clone(),
        profile: metadata.profile.clone(),
        variants,
    })
}

/// Returns the canonical skill target names in harness registry order.
#[must_use]
pub fn default_targets() -> Vec<String> {
    target::target_names()
}

fn capability_gaps(
    manifest: &Manifest,
    target_name: &str,
    metadata: &RenderMetadata,
) -> Result<(Vec<CapabilityGap>, Vec<CapabilityGap>), SkillError> {
    let known = [
        "subagents",
        "background_tasks",
        "mcp",
        "slash_commands",
        "hooks",
        "scheduled_tasks",
    ];
    let mut unsupported = Vec::new();
    let mut unknown = Vec::new();
    let Some(spec) = target::lookup(target_name) else {
        return Err(SkillError(format!("unknown target {target_name:?}")));
    };
    for required in &manifest.skill.requires {
        if !known.contains(&required.as_str()) {
            return Err(SkillError(format!(
                "skill requires unknown capability {required:?}; known capabilities are [{}]",
                known.join(", ")
            )));
        }
        let state = metadata.capabilities.get(required).copied().or_else(|| {
            spec.capabilities
                .iter()
                .find(|(name, _)| *name == required)
                .map(|(_, supported)| *supported)
        });
        match state {
            Some(false) => unsupported.push(CapabilityGap {
                capability: required.clone(),
                state: "unsupported".into(),
            }),
            Some(true) => {}
            None => unknown.push(CapabilityGap {
                capability: required.clone(),
                state: "unknown".into(),
            }),
        }
    }
    Ok((unsupported, unknown))
}

fn describe_gaps(gaps: &[CapabilityGap]) -> String {
    let noun = if gaps.len() == 1 {
        "capability"
    } else {
        "capabilities"
    };
    format!(
        "{noun} {}",
        gaps.iter()
            .map(|gap| format!("{:?}", gap.capability))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn filter_and_merge_metadata(
    frontmatter: &mut Frontmatter,
    target_name: &str,
    config: &BTreeMap<String, String>,
) {
    let mut metadata = match std::mem::take(&mut frontmatter.metadata) {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    let targets = target::target_names().into_iter().collect::<BTreeSet<_>>();
    metadata.retain(|key, _| !targets.contains(key) || key == target_name);
    for (key, value) in config {
        metadata.insert(key.clone(), Value::String(value.clone()));
    }
    frontmatter.metadata = Value::Object(metadata);
}

fn apply_frontmatter_overlay(
    bundle: &Bundle,
    target_name: &str,
    frontmatter: &mut Frontmatter,
) -> Result<(), SkillError> {
    let path = format!("overlays/{target_name}/frontmatter.toml");
    let raw = match read_bundle_bytes(bundle, Path::new(&path), &path, MAX_INPUT_SIZE) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| SkillError(format!("invalid_utf8_overlay: {path}")))?,
        Err(error) if error.0.contains("No such file") || error.0.contains("not found") => {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let document = raw
        .parse::<DocumentMut>()
        .map_err(|error| SkillError(format!("parse {path}: {error}")))?;
    match document.get("name").and_then(Item::as_str) {
        Some(value) if !value.is_empty() => value.clone_into(&mut frontmatter.name),
        _ => {}
    }
    match document.get("description").and_then(Item::as_str) {
        Some(value) if !value.is_empty() => value.clone_into(&mut frontmatter.description),
        _ => {}
    }
    match document.get("compatibility").and_then(Item::as_str) {
        Some(value) if !value.is_empty() => value.clone_into(&mut frontmatter.compatibility),
        _ => {}
    }
    if let Some(table) = document.get("metadata").and_then(Item::as_table_like) {
        let metadata = frontmatter
            .metadata
            .as_object_mut()
            .expect("metadata is object");
        for (key, value) in table.iter() {
            metadata.insert(key.to_owned(), toml_item_to_json(value));
        }
    }
    Ok(())
}

fn toml_item_to_json(item: &Item) -> Value {
    if let Some(value) = item.as_str() {
        Value::String(value.to_owned())
    } else if let Some(value) = item.as_integer() {
        Value::Number(value.into())
    } else if let Some(value) = item.as_float() {
        serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
    } else if let Some(value) = item.as_bool() {
        Value::Bool(value)
    } else if let Some(array) = item.as_array() {
        Value::Array(array.iter().map(toml_value_to_json).collect())
    } else if let Some(table) = item.as_table_like() {
        Value::Object(
            table
                .iter()
                .map(|(key, value)| (key.to_owned(), toml_item_to_json(value)))
                .collect(),
        )
    } else {
        Value::Null
    }
}

fn toml_value_to_json(value: &toml_edit::Value) -> Value {
    if let Some(value) = value.as_str() {
        Value::String(value.to_owned())
    } else if let Some(value) = value.as_integer() {
        Value::Number(value.into())
    } else if let Some(value) = value.as_float() {
        serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
    } else if let Some(value) = value.as_bool() {
        Value::Bool(value)
    } else if let Some(array) = value.as_array() {
        Value::Array(array.iter().map(toml_value_to_json).collect())
    } else {
        Value::Null
    }
}

fn compose_body(
    bundle: &Bundle,
    target_name: &str,
    config: &TargetConfig,
) -> Result<String, SkillError> {
    let prepend = overlay_text(bundle, target_name, "prepend.md", &config.prepend)?;
    let append = overlay_text(bundle, target_name, "append.md", &config.append)?;
    let mut parts = Vec::new();
    if !prepend.trim().is_empty() {
        parts.push(prepend.trim_end_matches('\n').to_owned());
    }
    parts.push(bundle.body.trim_end_matches('\n').to_owned());
    if !append.trim().is_empty() {
        parts.push(append.trim_end_matches('\n').to_owned());
    }
    Ok(format!("{}\n", parts.join("\n\n")))
}

fn overlay_text(
    bundle: &Bundle,
    target_name: &str,
    default_name: &str,
    configured: &str,
) -> Result<String, SkillError> {
    let path = if configured.is_empty() {
        format!("overlays/{target_name}/{default_name}")
    } else {
        let path = Path::new(configured);
        if path.is_absolute()
            || path
                .components()
                .any(|component| component == std::path::Component::ParentDir)
        {
            return Err(SkillError(format!(
                "overlay reference {configured:?} escapes skill root"
            )));
        }
        path.to_string_lossy().replace('\\', "/")
    };
    match read_bundle_bytes(bundle, Path::new(&path), &path, MAX_INPUT_SIZE) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| SkillError(format!("invalid_utf8_overlay: {path}"))),
        Err(error) if error.0.contains("No such file") || error.0.contains("not found") => {
            Ok(String::new())
        }
        Err(error) => Err(error),
    }
}
