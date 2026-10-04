//! UI reads hydrate through the existing Store and Go-compatible row renderers.

use super::{Server, wire};
use crate::{SearchRow, StoreError, search_rows::SEARCH_COLUMNS};
use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) fn query(raw: Option<&str>) -> BTreeMap<String, String> {
    let decode = |text: &str| {
        percent_encoding::percent_decode_str(&text.replace('+', " "))
            .decode_utf8_lossy()
            .into_owned()
    };
    let mut values = BTreeMap::new();
    for pair in raw.unwrap_or_default().split('&') {
        if pair.is_empty() || pair.contains(';') {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        values.entry(decode(key)).or_insert_with(|| decode(value));
    }
    values
}

pub(super) fn array(rows: &[String]) -> String {
    if rows.is_empty() {
        "null".into()
    } else {
        format!("[{}]", rows.join(","))
    }
}

fn supported(values: &BTreeMap<String, String>, names: &[&str]) -> bool {
    values
        .iter()
        .all(|(key, value)| value.is_empty() || names.contains(&key.as_str()))
}

impl Server {
    pub(super) fn list(&self, params: &BTreeMap<String, String>) -> wire::Reply {
        if !supported(params, &["scope", "limit"]) {
            return wire::unsupported();
        }
        let limit = params
            .get("limit")
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(100)
            .clamp(1, 1000);
        let result = self.store.list_lite(
            params.get("scope").map_or("", String::as_str),
            usize::try_from(limit).unwrap_or(100),
        );
        match result {
            Ok(rows) => {
                // Full PII/policy read admission belongs to the remaining HTTP
                // cutover; refuse data this proven slice cannot safely render.
                if rows.iter().any(|row| {
                    !crate::direct_text_supported(&row.content)
                        || row
                            .metadata
                            .values()
                            .any(|v| !v.as_str().is_some_and(crate::direct_text_supported))
                }) {
                    return wire::unsupported();
                }
                wire::raw_json(
                    200,
                    array(
                        &rows
                            .iter()
                            .map(crate::MemoryListRow::to_go_json)
                            .collect::<Vec<_>>(),
                    ),
                )
            }
            Err(_) => wire::error(500, "INTERNAL_ERROR", "Failed to list memories"),
        }
    }

    pub(super) fn rules(&self, params: &BTreeMap<String, String>) -> wire::Reply {
        match self
            .store
            .list_rules(params.get("scope").map_or("", String::as_str))
        {
            Ok(rows)
                if rows.iter().any(|row| {
                    !crate::direct_text_supported(&row.content)
                        || row
                            .metadata
                            .values()
                            .any(|v| !v.as_str().is_some_and(crate::direct_text_supported))
                }) =>
            {
                wire::unsupported()
            }
            Ok(rows) => wire::raw_json(
                200,
                format!(
                    "{{\"rules\":{}}}",
                    array(
                        &rows
                            .iter()
                            .map(crate::RuleRow::to_go_json)
                            .collect::<Vec<_>>()
                    )
                ),
            ),
            Err(_) => wire::error(500, "INTERNAL_ERROR", "failed to list rules"),
        }
    }

    pub(super) fn entities(&self) -> wire::Reply {
        match self.store.entities() {
            Ok(mut rows) => {
                if rows.iter().any(|row| !safe_value(row)) {
                    return wire::unsupported();
                }
                for row in &mut rows {
                    if row["created_by"] == "" {
                        row.as_object_mut()
                            .expect("entity object")
                            .remove("created_by");
                    }
                    for key in ["created_at", "updated_at"] {
                        if let Some(time) = row
                            .get(key)
                            .and_then(Value::as_str)
                            .and_then(crate::gotime::parse)
                        {
                            row[key] = json!(crate::gojson::timestamp(Some(time)));
                        }
                    }
                }
                wire::raw_json(
                    200,
                    format!(
                        "{{\"entities\":{}}}",
                        if rows.is_empty() {
                            "null".into()
                        } else {
                            json!(rows).to_string()
                        }
                    ),
                )
            }
            Err(_) => wire::error(500, "INTERNAL_ERROR", "failed to list entities"),
        }
    }

    pub(super) fn get(&self, params: &BTreeMap<String, String>) -> wire::Reply {
        let id = params.get("id").map_or("", String::as_str);
        if id.is_empty() {
            return wire::error(400, "INVALID_REQUEST", "missing required parameter: id");
        }
        if !supported(params, &["id"]) {
            return wire::unsupported();
        }
        let result = (|| -> Result<Option<SearchRow>, StoreError> {
            let conn = self.store.lock()?;
            let sql = format!("SELECT {SEARCH_COLUMNS} FROM memories WHERE id=?");
            let row = conn.query_row(&sql, [id], SearchRow::from_row).optional()?;
            if row.as_ref().is_some_and(safe_row) {
                conn.execute("UPDATE memories SET access_count=access_count+1,prev_access=last_access,last_access=? WHERE id=?",rusqlite::params![crate::cli_write::timestamp(),id])?;
            }
            Ok(row)
        })();
        match result {
            Ok(Some(row)) if safe_row(&row) => wire::raw_json(200, full_json(&row)),
            Ok(Some(_)) => wire::unsupported(),
            Ok(None) => wire::error(404, "NOT_FOUND", &format!("memory not found: {id}")),
            Err(_) => wire::error(500, "INTERNAL_ERROR", "failed to fetch memory"),
        }
    }
}

pub(super) fn full_json(row: &SearchRow) -> String {
    let mut value = row.to_go_json();
    if !row.embedding.is_empty() {
        let vector = row
            .embedding
            .iter()
            .map(|v| crate::gojson::number_f32(*v))
            .collect::<Vec<_>>()
            .join(",");
        value.insert_str(1, &format!("\"embedding\":[{vector}],"));
    }
    value
}

pub(super) fn safe_row(row: &SearchRow) -> bool {
    crate::direct_text_supported(&row.content)
        && row
            .metadata
            .values()
            .all(|value| value.as_str().is_some_and(crate::direct_text_supported))
}

// Reject values needing the unported HTTP redaction policy before exposing data.
fn safe_value(value: &Value) -> bool {
    match value {
        Value::String(text) => crate::direct_text_supported(text),
        Value::Array(items) => items.iter().all(safe_value),
        Value::Object(items) => items.values().all(safe_value),
        _ => true,
    }
}
