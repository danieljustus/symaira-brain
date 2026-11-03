//! Read routes hydrate through the existing Store and Go-compatible row
//! renderers; every memory leaving the process passes the complete shipped
//! PII guard (`security.RedactMemory`).

use super::{Server, wire};
use crate::{SearchRow, StoreError, search_rows::SEARCH_COLUMNS};
use rusqlite::OptionalExtension;
use serde_json::Value;
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

fn rank(table: &[(&str, i32)], name: &str, default: i32) -> i32 {
    table
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map_or(default, |(_, rank)| *rank)
}

/// `db.PassesPolicyFilter` over the stored string metadata.
pub(super) fn passes_policy(
    metadata: &serde_json::Map<String, Value>,
    max_sensitivity: &str,
    min_sharing: &str,
    client: &str,
) -> bool {
    if max_sensitivity.is_empty() && min_sharing.is_empty() && client.is_empty() {
        return true;
    }
    let get = |key: &str| metadata.get(key).and_then(Value::as_str).unwrap_or_default();
    if !max_sensitivity.is_empty() {
        let actual = match get("sensitivity") {
            "" => "internal",
            other => other,
        };
        let levels = [("public", 0), ("internal", 1), ("confidential", 2), ("secret", 3)];
        if rank(&levels, actual, 1) > rank(&levels, max_sensitivity, 1) {
            return false;
        }
    }
    if !min_sharing.is_empty() {
        let actual = match get("sharing_level") {
            "" => "private",
            other => other,
        };
        let levels = [("private", 0), ("team", 1), ("org", 2), ("public", 3)];
        if rank(&levels, actual, 0) < rank(&levels, min_sharing, 0) {
            return false;
        }
    }
    if !client.is_empty() {
        let csv = |value: &str| {
            value
                .split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        if csv(get("denied_clients")).iter().any(|c| c == client) {
            return false;
        }
        let allowed = get("allowed_clients");
        if !allowed.is_empty() && !csv(allowed).iter().any(|c| c == client) {
            return false;
        }
    }
    true
}

impl Server {
    pub(super) fn list(&self, params: &BTreeMap<String, String>) -> wire::Reply {
        let limit = params
            .get("limit")
            .and_then(|raw| {
                let digits = raw.strip_prefix(['+', '-']).unwrap_or(raw);
                (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
                    .then(|| raw.parse::<i64>().ok())
                    .flatten()
            })
            .unwrap_or(100)
            .clamp(1, 1000);
        let get = |name: &str| params.get(name).map_or("", String::as_str);
        let result = self
            .store
            .list_lite(get("scope"), usize::try_from(limit).unwrap_or(100));
        match result {
            Ok(rows) => {
                let mut rows = rows
                    .into_iter()
                    .filter(|row| {
                        passes_policy(
                            &row.metadata,
                            get("max_sensitivity"),
                            get("min_sharing_level"),
                            get("client_id"),
                        )
                    })
                    .collect::<Vec<_>>();
                for row in &mut rows {
                    row.redact_pii();
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
            Ok(rows) => {
                let rendered = rows
                    .iter()
                    .map(|row| {
                        let text = |key: &str| row[key].as_str().unwrap_or_default().to_owned();
                        let time = |key: &str| {
                            crate::gotime::parse(&text(key)).map_or_else(
                                || text(key),
                                |at| crate::gojson::timestamp(Some(at)).unwrap_or_default(),
                            )
                        };
                        // Field order of the shipped `db.Entity` struct.
                        let mut members = crate::gojson::Members::default();
                        members.string("id", &text("id"));
                        members.string("name", &text("name"));
                        members.string("type", &text("type"));
                        members.raw("aliases", &row["aliases"].to_string());
                        members.string("description", &text("description"));
                        members.string_if_present("created_by", &text("created_by"));
                        members.string("created_at", &time("created_at"));
                        members.string("updated_at", &time("updated_at"));
                        members.finish()
                    })
                    .collect::<Vec<_>>();
                wire::raw_json(200, format!("{{\"entities\":{}}}", array(&rendered)))
            }
            Err(_) => wire::error(500, "INTERNAL_ERROR", "failed to list entities"),
        }
    }

    pub(super) fn get(&self, params: &BTreeMap<String, String>) -> wire::Reply {
        let id = params.get("id").map_or("", String::as_str);
        if id.is_empty() {
            return wire::error(400, "INVALID_REQUEST", "missing required parameter: id");
        }
        let result = (|| -> Result<Option<SearchRow>, StoreError> {
            let conn = self.store.lock()?;
            let sql = format!("SELECT {SEARCH_COLUMNS} FROM memories WHERE id=?");
            let row = conn.query_row(&sql, [id], SearchRow::from_row).optional()?;
            if row.is_some() {
                conn.execute("UPDATE memories SET access_count=access_count+1,prev_access=last_access,last_access=? WHERE id=?",rusqlite::params![crate::cli_write::timestamp(),id])?;
            }
            Ok(row)
        })();
        match result {
            Ok(Some(mut row)) => {
                let client = params.get("client_id").map_or("", String::as_str);
                if !client.is_empty() && !passes_policy(&row.metadata, "", "", client) {
                    return wire::error(
                        403,
                        "FORBIDDEN",
                        &format!("access denied: memory {id} is not accessible by client {client}"),
                    );
                }
                row.redact_pii();
                wire::raw_json(200, row.to_go_json_embedded())
            }
            Ok(None) => wire::error(404, "NOT_FOUND", &format!("memory not found: {id}")),
            Err(_) => wire::error(500, "INTERNAL_ERROR", "failed to fetch memory"),
        }
    }
}
