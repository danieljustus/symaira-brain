//! Read-only marker decoding; future schemas never acquire write permission.
use super::marker::{MARKER_FILE, Marker};
use super::marker_string::repair_json_strings;
use crate::SkillError;
use cap_std::fs::Dir;
use std::path::Path;

pub(super) struct Observation {
    pub(super) marker: Marker,
    pub(super) error: Option<String>,
}

pub(super) fn read(root: &Dir) -> Result<Option<Observation>, SkillError> {
    match root.symlink_metadata(MARKER_FILE) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(SkillError(error.to_string())),
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Ok(Some(Observation {
                marker: empty(),
                error: Some("marker is a symlink".to_owned()),
            }));
        }
        Ok(_) => {}
    }
    let bytes = crate::load::read_limited_nofollow(
        root,
        Path::new(MARKER_FILE),
        "marker",
        crate::MAX_INPUT_SIZE,
    )?;
    Ok(Some(decode(&bytes)))
}

fn empty() -> Marker {
    Marker {
        schema_version: 0,
        managed_by: String::new(),
        target: String::new(),
        name: String::new(),
        rendered_at: crate::GoText::default(),
        mode: String::new(),
        installed: String::new(),
        source_hash: String::new(),
        allow_executable: false,
        extra: std::collections::BTreeMap::default(),
    }
}

fn decode(bytes: &[u8]) -> Observation {
    let mut result = Observation {
        marker: empty(),
        error: None,
    };
    let fields = match super::marker_json::fields(bytes) {
        Ok(fields) => fields,
        Err(error) => {
            result.error = Some(error);
            return result;
        }
    };
    let bytes = bytes.trim_ascii();
    if bytes == b"null" {
        return result;
    }
    if bytes.first() != Some(&b'{') {
        result.error = Some(format!(
            "json: cannot unmarshal {} into Go value of type install.Marker",
            value_type(bytes)
        ));
        return result;
    }
    for (key, raw) in fields {
        let Ok(key) = serde_json::from_str::<String>(&repair_json_strings(key)) else {
            continue;
        };
        let key: String = key
            .chars()
            .map(|ch| match ch {
                '\u{212a}' => 'k',
                '\u{017f}' => 's',
                _ => ch.to_ascii_lowercase(),
            })
            .collect();
        let raw = raw.trim_ascii();
        if raw == b"null" {
            continue;
        }
        let mut rendered_at = String::new();
        let string = match key.as_str() {
            "rendered_at" => Some(&mut rendered_at),
            "managed_by" => Some(&mut result.marker.managed_by),
            "target" => Some(&mut result.marker.target),
            "name" => Some(&mut result.marker.name),

            "mode" => Some(&mut result.marker.mode),
            "installed" => Some(&mut result.marker.installed),
            "source_hash" => Some(&mut result.marker.source_hash),
            _ => None,
        };
        let ty = if let Some(field) = string {
            if let Ok(value) = serde_json::from_str::<String>(&repair_json_strings(raw)) {
                *field = value;
                if key == "rendered_at" {
                    result.marker.rendered_at = rendered_at.into();
                }
                continue;
            }
            match key.as_str() {
                "target" => "render.Target",
                "mode" => "install.Mode",
                _ => "string",
            }
        } else if key == "allow_executable" {
            if let Ok(value) = serde_json::from_slice::<bool>(raw) {
                result.marker.allow_executable = value;
                continue;
            }
            "bool"
        } else if key == "schema_version" {
            // Observation does not use the schema as an ownership selector.
            // Go's field is int, so negative/future versions remain readable.
            if serde_json::from_slice::<i64>(raw).is_ok() {
                continue;
            }
            "int"
        } else {
            continue;
        };
        if result.error.is_none() {
            let value = if value_type(raw) == "number" && ty == "int" {
                format!("number {}", String::from_utf8_lossy(raw))
            } else {
                value_type(raw).into()
            };
            result.error = Some(format!(
                "json: cannot unmarshal {value} into Go struct field Marker.{key} of type {ty}"
            ));
        }
    }
    result
}

fn value_type(raw: &[u8]) -> &'static str {
    match raw.first() {
        Some(b'"') => "string",
        Some(b'{') => "object",
        Some(b'[') => "array",
        Some(b't' | b'f') => "bool",
        Some(b'n') => "null",
        _ => "number",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn future_fields_are_observed_but_the_writer_remains_strict() {
        let raw = br#"{"schema_version":44,"mode":"copy","name":"demo"}"#;
        let row = decode(raw);
        assert!(row.error.is_none());
        assert_eq!(row.marker.name, "demo");
        assert!(matches!(
            super::super::marker::parse_marker(raw).unwrap(),
            super::super::marker::MarkerState::UnsupportedSchema(44)
        ));
    }
    #[test]
    fn type_errors_retain_other_fields_and_syntax_errors_do_not() {
        let row = decode(br#"{"mode":4,"name":"demo","installed":"time"}"#);
        assert_eq!(row.marker.name, "demo");
        assert_eq!(row.marker.installed, "time");
        assert_eq!(
            row.error.as_deref(),
            Some(
                "json: cannot unmarshal number into Go struct field Marker.mode of type install.Mode"
            )
        );
        let row = decode(br#"{"name":"demo",broken}"#);
        assert!(row.marker.name.is_empty());
        assert_eq!(
            row.error.as_deref(),
            Some("invalid character 'b' looking for beginning of object key string")
        );
    }
}
