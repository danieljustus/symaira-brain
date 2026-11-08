//! Frontmatter merge behind [`crate::pull`]: harness-side edits flow into the
//! library source, overlay-owned keys are refused.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use serde_yaml_ng::{Mapping, Value as Yaml};
use toml_edit::{DocumentMut, Item};

use crate::model::{Bundle, SkillError};
use crate::pull::{PullFrontmatterChange, PullResult};

/// Frontmatter keys the renderer can emit; any other key is not part of the
/// installed copy by construction, so its absence there is not a deletion.
const RENDERED_KEYS: &[&str] = &[
    "name",
    "description",
    "category",
    "version",
    "author",
    "license",
    "compatibility",
    "platforms",
    "required_environment_variables",
    "metadata",
];

fn overlay_dir(target: &str) -> String {
    format!("overlays/{target}")
}

fn toml_to_yaml(item: &Item) -> Yaml {
    if let Some(value) = item.as_str() {
        Yaml::String(value.to_owned())
    } else if let Some(value) = item.as_integer() {
        Yaml::Number(value.into())
    } else if let Some(value) = item.as_float() {
        Yaml::Number(value.into())
    } else if let Some(value) = item.as_bool() {
        Yaml::Bool(value)
    } else if let Some(array) = item.as_array() {
        Yaml::Sequence(
            array
                .iter()
                .map(|value| toml_to_yaml(&Item::Value(value.clone())))
                .collect(),
        )
    } else if let Some(table) = item.as_table_like() {
        let mut map = Mapping::new();
        for (key, value) in table.iter() {
            map.insert(Yaml::String(key.to_owned()), toml_to_yaml(value));
        }
        Yaml::Mapping(map)
    } else {
        Yaml::Null
    }
}

/// Keys whose installed value the target overlay controls, with the value the
/// renderer synthesizes for each.
fn owned_keys(bundle: &Bundle, target: &str) -> Result<BTreeMap<String, Yaml>, SkillError> {
    let mut owned = BTreeMap::from([("compatibility".to_owned(), Yaml::String(target.to_owned()))]);
    let config = bundle
        .manifest
        .targets
        .get(target)
        .cloned()
        .unwrap_or_default();
    if !config.alias.is_empty() {
        owned.insert("name".into(), Yaml::String(config.alias.clone()));
    }
    if !config.description.is_empty() {
        owned.insert(
            "description".into(),
            Yaml::String(config.description.clone()),
        );
    }
    for (key, value) in &config.metadata {
        owned.insert(format!("metadata.{key}"), Yaml::String(value.clone()));
    }
    let path = bundle
        .root
        .join(overlay_dir(target))
        .join("frontmatter.toml");
    if let Ok(raw) = fs::read_to_string(&path) {
        let document = raw
            .parse::<DocumentMut>()
            .map_err(|error| SkillError(format!("parse {}: {error}", path.display())))?;
        for (key, value) in document.iter() {
            if key == "metadata" {
                if let Some(table) = value.as_table_like() {
                    for (sub, value) in table.iter() {
                        owned.insert(format!("metadata.{sub}"), toml_to_yaml(value));
                    }
                }
            } else {
                owned.insert(key.to_owned(), toml_to_yaml(value));
            }
        }
    }
    Ok(owned)
}

fn json(value: Option<&Yaml>) -> Option<serde_json::Value> {
    value.and_then(|value| serde_json::to_value(value).ok())
}

fn lookup<'a>(map: &'a Mapping, key: &str) -> Option<&'a Yaml> {
    map.get(Yaml::String(key.to_owned()))
}

/// Outcome of one key of the frontmatter merge.
enum Decision {
    /// Keep the library value (equal, refused or not a harness edit).
    Keep,
    /// Take the installed value.
    Set(Yaml),
    /// The harness removed the key.
    Remove,
}

/// Key-by-key frontmatter merge state.
struct Merge<'a> {
    owned: &'a BTreeMap<String, Yaml>,
    result: &'a mut PullResult,
    other_targets: BTreeSet<String>,
}

impl Merge<'_> {
    /// Decides what to do with one key and records the change.
    fn decide(
        &mut self,
        key: &str,
        source: Option<&Yaml>,
        installed: Option<&Yaml>,
        droppable: bool,
    ) -> Decision {
        if source == installed || (installed.is_none() && droppable) {
            // Equal, or a key the renderer never emits: not a harness edit.
            return Decision::Keep;
        }
        if let Some(synthesized) = self.owned.get(key) {
            if Some(synthesized) != installed {
                self.result.refusals.push(format!(
                    "frontmatter key {key:?} is owned by target overlay"
                ));
            }
            return Decision::Keep;
        }
        let reason = if matches!(key, "allowed-tools" | "allowed_tools") {
            "permission-relevant frontmatter"
        } else {
            "frontmatter"
        };
        self.result.frontmatter_changes.push(PullFrontmatterChange {
            key: key.to_owned(),
            from: json(source),
            to: json(installed),
            reason: reason.to_owned(),
        });
        installed.cloned().map_or(Decision::Remove, Decision::Set)
    }
}

/// Merges harness-side frontmatter edits into `source`; overlay-owned keys
/// that differ from what the renderer synthesizes are refused.
pub(crate) fn pull_frontmatter(
    bundle: &Bundle,
    target: &str,
    source: &mut Mapping,
    installed: &Mapping,
    result: &mut PullResult,
) -> Result<(), SkillError> {
    let owned = owned_keys(bundle, target)?;
    let other_targets = crate::target_names()
        .into_iter()
        .filter(|name| name != target)
        .collect();
    let mut merge = Merge {
        owned: &owned,
        result,
        other_targets,
    };
    let as_map = |value: Option<&Yaml>| match value {
        Some(Yaml::Mapping(map)) => map.clone(),
        _ => Mapping::new(),
    };
    let mut source_meta = as_map(lookup(source, "metadata"));
    let installed_meta = as_map(lookup(installed, "metadata"));
    let meta_keys: BTreeSet<String> = source_meta
        .keys()
        .chain(installed_meta.keys())
        .filter_map(|key| key.as_str().map(str::to_owned))
        .collect();
    for key in &meta_keys {
        let droppable = merge.other_targets.contains(key);
        let full = format!("metadata.{key}");
        let kept = merge.decide(
            &full,
            lookup(&source_meta, key),
            lookup(&installed_meta, key),
            droppable,
        );
        match kept {
            Decision::Set(value) => {
                source_meta.insert(Yaml::String(key.clone()), value);
            }
            Decision::Remove => {
                source_meta.remove(Yaml::String(key.clone()));
            }
            Decision::Keep => {}
        }
    }
    let keys: BTreeSet<String> = source
        .keys()
        .chain(installed.keys())
        .filter_map(|key| key.as_str().map(str::to_owned))
        .filter(|key| key != "metadata")
        .collect();
    for key in &keys {
        let droppable = !RENDERED_KEYS.contains(&key.as_str());
        let kept = merge.decide(key, lookup(source, key), lookup(installed, key), droppable);
        match kept {
            Decision::Set(value) => {
                source.insert(Yaml::String(key.clone()), value);
            }
            Decision::Remove => {
                source.remove(Yaml::String(key.clone()));
            }
            Decision::Keep => {}
        }
    }
    if source_meta.is_empty() {
        source.remove(Yaml::String("metadata".into()));
    } else if merge
        .result
        .frontmatter_changes
        .iter()
        .any(|c| c.key.starts_with("metadata."))
    {
        source.insert(Yaml::String("metadata".into()), Yaml::Mapping(source_meta));
    }
    if !merge.result.refusals.is_empty() {
        let message = format!("pull refused: {}", merge.result.refusals.join("; "));
        return Err(SkillError(message));
    }
    merge
        .result
        .frontmatter_changes
        .sort_by(|left, right| left.key.cmp(&right.key));
    Ok(())
}
