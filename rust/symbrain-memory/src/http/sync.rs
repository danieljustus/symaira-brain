//! `/api/sync/changes`, `/api/sync/apply` and `/api/sync/relay`.

use super::{Server, auth::Claims, go_json, rfc3339, sync_decode, wire};
use crate::gojson::{self, Members};
use crate::serve_store::{ApplyMode, RelayBlob, SyncRow};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, SecondsFormat, Utc};
use hyper::Method;
use std::collections::BTreeMap;

fn parse_go_int(text: &str) -> Option<i64> {
    // strconv.Atoi: optional sign, ASCII digits only, fits in an int.
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<i64>().ok()
}

fn rfc3339_nano(time: DateTime<Utc>) -> String {
    gojson::timestamp(Some(time)).unwrap_or_default()
}

fn array(rows: &[String]) -> String {
    if rows.is_empty() {
        "null".into()
    } else {
        format!("[{}]", rows.join(","))
    }
}

fn policy_passes(row: &SyncRow, params: &BTreeMap<String, String>) -> bool {
    let get = |name: &str| params.get(name).map_or("", String::as_str);
    super::read::passes_policy(
        row.metadata(),
        get("max_sensitivity"),
        get("min_sharing_level"),
        get("client_id"),
    )
}

impl Server {
    #[allow(clippy::too_many_lines)] // One request: parse, page, filter, render.
    pub(super) fn sync_changes(
        &self,
        method: &Method,
        params: &BTreeMap<String, String>,
    ) -> wire::Reply {
        if method != Method::GET {
            return wire::error(405, "METHOD_NOT_ALLOWED", "Method not allowed");
        }
        let mut since = crate::zero_time();
        if let Some(raw) = params.get("since").filter(|raw| !raw.is_empty()) {
            let Some(parsed) = rfc3339::parse(raw) else {
                return wire::error_logged(
                    400,
                    "INVALID_REQUEST",
                    "invalid since parameter; expected RFC3339",
                    &go_json::time_error(raw, go_json::RFC3339).unwrap_or_default(),
                );
            };
            since = parsed;
        }
        if let Some(cursor) = params.get("cursor").filter(|raw| !raw.is_empty()) {
            let Ok(decoded) = STANDARD.decode(cursor) else {
                return wire::error_logged(
                    400,
                    "INVALID_REQUEST",
                    "invalid cursor parameter",
                    &format!(
                        "illegal base64 data at input byte {}",
                        go_json::base64_error(cursor, false).unwrap_or_default()
                    ),
                );
            };
            let text = String::from_utf8_lossy(&decoded);
            let Some(parsed) = std::str::from_utf8(&decoded).ok().and_then(rfc3339::parse) else {
                return wire::error_logged(
                    400,
                    "INVALID_REQUEST",
                    "invalid cursor format",
                    &go_json::time_error(&text, go_json::RFC3339_NANO).unwrap_or_default(),
                );
            };
            since = parsed;
        }
        let mut limit = 500_i64;
        if let Some(value) = params.get("limit").and_then(|raw| parse_go_int(raw))
            && value > 0
        {
            limit = value;
        }
        limit = limit.clamp(1, 10000);
        let full = params
            .get("include_embeddings")
            .is_some_and(|value| value == "true");
        let Ok(rows) = self.store.sync_changes(since, limit + 1, full) else {
            return wire::error(500, "INTERNAL_ERROR", "Failed to fetch changes");
        };
        let mut rows = if ["max_sensitivity", "min_sharing_level", "client_id"]
            .iter()
            .any(|name| params.get(*name).is_some_and(|value| !value.is_empty()))
        {
            rows.into_iter()
                .filter(|row| policy_passes(row, params))
                .collect()
        } else {
            rows
        };
        let limit_len = usize::try_from(limit).unwrap_or(usize::MAX);
        let mut next_cursor = None;
        if rows.len() > limit_len {
            rows.truncate(limit_len);
            if let Some(updated) = rows.last().and_then(SyncRow::updated_at) {
                next_cursor = Some(STANDARD.encode(rfc3339_nano(updated)));
            }
        }
        for row in &mut rows {
            row.redact_pii();
        }
        let Ok(deleted) = self.store.deleted_since(since) else {
            return wire::error(500, "INTERNAL_ERROR", "Failed to fetch tombstones");
        };
        let mut members = Members::default();
        members.raw(
            "deleted",
            &array(
                &deleted
                    .iter()
                    .map(|(id, at)| {
                        let mut entry = Members::default();
                        entry.string("id", id);
                        entry.string("deleted_at", &rfc3339_nano(*at));
                        entry.finish()
                    })
                    .collect::<Vec<_>>(),
            ),
        );
        members.raw(
            "memories",
            &array(&rows.iter().map(SyncRow::to_json).collect::<Vec<_>>()),
        );
        if let Some(cursor) = next_cursor {
            members.string("next_cursor", &cursor);
        }
        members.string(
            "server_time",
            &Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        );
        wire::raw_json(200, members.finish())
    }

    pub(super) fn sync_apply(&self, method: &Method, body: &[u8], claims: &Claims) -> wire::Reply {
        if method != Method::POST {
            return wire::error(405, "METHOD_NOT_ALLOWED", "Method not allowed");
        }
        let Some(request) = sync_decode::apply(body) else {
            return wire::error_logged(
                400,
                "INVALID_REQUEST",
                "Bad request body",
                &go_json::unmarshal_error(body, &go_json::Shape::Apply),
            );
        };
        let mode = ApplyMode {
            quantize_binary: self.options.quantize_binary,
            audit_enabled: self.options.audit_enabled,
        };
        let (mut applied, mut skipped, mut invalid_scope, mut invalid_id, mut deleted) =
            (0_i64, 0_i64, 0_i64, 0_i64, 0_i64);
        for mut memory in request.memories {
            if memory.id.is_empty() {
                skipped += 1;
                continue;
            }
            if !sync_decode::uuid_ok(&memory.id) {
                invalid_id += 1;
                continue;
            }
            if !(memory.scope.is_empty()
                || ["global", "project", "agent", "user", "session"]
                    .contains(&memory.scope.as_str()))
            {
                invalid_scope += 1;
                continue;
            }
            // The service redacts content and metadata on every ingest.
            memory.content = crate::pii::redact(&memory.content);
            memory.metadata = memory.metadata.as_ref().map(crate::pii::redact_map);
            match self.store.serve_sync_upsert(&memory, mode) {
                Ok(true) => applied += 1,
                Ok(false) => skipped += 1,
                Err(_) => return wire::error(500, "INTERNAL_ERROR", "Failed to apply memory"),
            }
        }
        for (id, at) in request.deleted {
            if id.is_empty() {
                continue;
            }
            if !sync_decode::uuid_ok(&id) {
                invalid_id += 1;
                continue;
            }
            match self
                .store
                .serve_apply_delete(&id, at, self.options.audit_enabled)
            {
                Ok(true) => deleted += 1,
                Ok(false) => {}
                Err(_) => return wire::error(500, "INTERNAL_ERROR", "Failed to apply delete"),
            }
        }
        if self.options.audit_enabled {
            let _ = self.store.lock().map(|conn| {
                crate::cli_write::audit(
                    &conn,
                    "sync",
                    "",
                    "",
                    "",
                    &claims.sub,
                    &format!(
                        "applied={applied} skipped={skipped} deleted={deleted} invalidScope={invalid_scope} invalidID={invalid_id}"
                    ),
                );
            });
        }
        // Go renders map[string]int in byte order: the legacy camelCase keys
        // sort before their snake_case twins.
        wire::raw_json(
            200,
            format!(
                "{{\"applied\":{applied},\"deleted\":{deleted},\"skipped\":{skipped},\"skippedInvalidID\":{invalid_id},\"skippedInvalidScope\":{invalid_scope},\"skipped_invalid_id\":{invalid_id},\"skipped_invalid_scope\":{invalid_scope}}}"
            ),
        )
    }

    pub(super) fn sync_relay(
        &self,
        method: &Method,
        params: &BTreeMap<String, String>,
        body: &[u8],
    ) -> wire::Reply {
        if method == Method::GET {
            let mut since = crate::zero_time();
            if let Some(raw) = params.get("since").filter(|raw| !raw.is_empty()) {
                let Some(parsed) = rfc3339::parse(raw) else {
                    return wire::error_logged(
                        400,
                        "INVALID_REQUEST",
                        "invalid since parameter; expected RFC3339",
                        &go_json::time_error(raw, go_json::RFC3339_NANO).unwrap_or_default(),
                    );
                };
                since = parsed;
            }
            let mut limit = 500_i64;
            if let Some(value) = params.get("limit").and_then(|raw| parse_go_int(raw))
                && value > 0
                && value <= 10000
            {
                limit = value;
            }
            let Ok(blobs) = self.store.relay_blobs_since(since, limit) else {
                return wire::error(500, "INTERNAL_ERROR", "Failed to fetch relay blobs");
            };
            let rendered = blobs
                .iter()
                .map(|blob| {
                    let mut entry = Members::default();
                    entry.string("id", &blob.id);
                    entry.string("updated_at", &rfc3339_nano(blob.updated_at));
                    entry.string("blob", &STANDARD.encode(&blob.blob));
                    entry.finish()
                })
                .collect::<Vec<_>>();
            let mut members = Members::default();
            members.raw("blobs", &array(&rendered));
            members.string("server_time", &rfc3339_nano(Utc::now()));
            return wire::raw_json(200, members.finish());
        }
        if method != Method::POST {
            return wire::error(405, "METHOD_NOT_ALLOWED", "Method not allowed");
        }
        let Some(blobs) = sync_decode::relay(body) else {
            return wire::error_logged(
                400,
                "INVALID_REQUEST",
                "Bad request body",
                &go_json::unmarshal_error(body, &go_json::Shape::Relay),
            );
        };
        let (mut stored, mut skipped) = (0, 0);
        for blob in blobs {
            if blob.id.is_empty() || blob.data.is_empty() {
                skipped += 1;
                continue;
            }
            let blob = RelayBlob {
                id: blob.id,
                updated_at: blob.updated_at,
                blob: blob.data,
            };
            match self.store.store_relay_blob(&blob) {
                Ok(true) => stored += 1,
                Ok(false) => skipped += 1,
                Err(_) => {
                    return wire::error(500, "INTERNAL_ERROR", "Failed to store relay blob");
                }
            }
        }
        wire::raw_json(
            200,
            format!("{{\"skipped\":{skipped},\"stored\":{stored}}}"),
        )
    }
}
