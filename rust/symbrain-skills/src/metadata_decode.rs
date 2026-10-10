//! Numeric-only frozen Go Frontmatter normalization, with strings untouched.
use serde_json::Value;

use super::SkillError;

pub(super) fn normalize(value: &mut Value) -> Result<(), SkillError> {
    match value {
        Value::Number(number) if number.is_f64() => {
            // Go's old map marshal/reparse turns integral decimals (including
            // genuine -0.0) into integers; scientific scalars remain floats.
            let scalar = serde_yaml_ng_skill_encoder::to_string(number)
                .map_err(|error| SkillError(format!("normalize metadata number: {error}")))?;
            *value = serde_yaml_ng::from_str(&scalar)
                .map_err(|error| SkillError(format!("normalize metadata number: {error}")))?;
        }
        Value::Array(items) => {
            for item in items {
                normalize(item)?;
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                normalize(item)?;
            }
        }
        _ => {}
    }
    Ok(())
}
