//! HTTP mutations retain service provenance, profile authorization and Store
//! ownership. Writes whose shipped pipeline is not native (secondary-fact
//! extraction, a write-path conflict, a non-canonical stored row) answer an
//! explicit `501` before any state changes.

use super::{Server, auth::Claims, go_json, read, sync_decode, wire};
use crate::DirectWrite;
use crate::cli_write::ServeExtra;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rusqlite::OptionalExtension;
use serde_json::json;
use std::collections::BTreeMap;

fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let (mut dot, mut na, mut nb) = (0.0_f64, 0.0_f64, 0.0_f64);
    for (x, y) in a.iter().zip(b) {
        dot += f64::from(*x) * f64::from(*y);
        na += f64::from(*x) * f64::from(*x);
        nb += f64::from(*y) * f64::from(*y);
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// `security.ExtractJTI`: the jti claim of an unverified token, or the
/// shipped failure wording.
fn extract_jti(token: &str) -> Result<String, String> {
    let parts = token.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err("invalid token structure".into());
    }
    // Go pads to a multiple of four and decodes with the padded URL alphabet.
    let mut padded = parts[1].to_owned();
    if padded.len() % 4 != 0 {
        padded.push_str(&"=".repeat(4 - padded.len() % 4));
    }
    if let Some(position) = go_json::base64_error(&padded, true) {
        return Err(format!(
            "invalid token payload: illegal base64 data at input byte {position}"
        ));
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(padded.trim_end_matches('='))
        .map_err(|error| format!("invalid token payload: {error}"))?;
    let claims = serde_json::from_slice::<super::auth::Claims>(&decoded).map_err(|error| {
        format!(
            "invalid token payload: {}",
            go_json::syntax_error(&decoded).unwrap_or_else(|| error.to_string())
        )
    })?;
    if claims.jti.is_empty() {
        return Err("token has no jti claim".into());
    }
    Ok(claims.jti)
}

struct Revoke {
    token: String,
    jti: String,
}

impl Server {
    pub(super) fn set(&self, body: &[u8], claims: &Claims) -> wire::Reply {
        let Some(input) = sync_decode::set_args(body) else {
            return wire::error_logged(
                400,
                "INVALID_REQUEST",
                "Bad request body",
                &go_json::unmarshal_error(body, &go_json::Shape::Set),
            );
        };
        if !self.options.direct_writes {
            return wire::unsupported();
        }
        let scope = if input.scope.is_empty() {
            "global".to_owned()
        } else {
            input.scope.clone()
        };
        if !["global", "project", "agent", "user", "session"].contains(&scope.as_str()) {
            return wire::error_logged(
                500,
                "INTERNAL_ERROR",
                "Failed to save memory",
                &format!(
                    "invalid scope {}: must be one of global, project, agent, user, session",
                    go_json::go_quote(&scope)
                ),
            );
        }
        let (content, mut labels) = crate::pii::redact_with_labels(&input.content);
        for label in &labels {
            super::log::warn(
                "PII redacted from memory content",
                &[("pattern", (*label).to_owned())],
            );
        }
        let mut metadata = BTreeMap::new();
        for (key, value) in &input.metadata {
            let (clean, found) = crate::pii::redact_with_labels(value);
            for label in &found {
                super::log::warn(
                    "PII redacted from memory metadata",
                    &[("pattern", (*label).to_owned())],
                );
            }
            labels.extend(found);
            metadata.insert(key.clone(), clean);
        }
        // Secondary-fact extraction is not native: refuse before any write.
        if !crate::direct_content_supported(&content)
            || metadata
                .values()
                .any(|value| !crate::direct_text_supported(value))
        {
            return wire::unsupported();
        }
        let embedding = self.embed(&content);
        if self.options.conflict_enabled && !input.working {
            match self.conflict_free(&content, &scope, &embedding) {
                Some(true) => {}
                Some(false) => return wire::unsupported(),
                None => {}
            }
        }
        let request = DirectWrite {
            content,
            scope: input.scope,
            metadata,
            author: claims.sub.clone(),
            kind: String::new(),
            entities: input.entities,
            staged: false,
            quantize_binary: self.options.quantize_binary,
            conflict_enabled: false,
        };
        let extra = ServeExtra {
            session: input.session_id.clone(),
            working_ttl: input
                .working
                .then(|| chrono::Duration::from_std(self.options.working_ttl).ok())
                .flatten(),
            embedding: Some(embedding),
        };
        match self.store.set_direct_inner(
            &request,
            &self.generator,
            "http",
            None,
            self.options.audit_enabled,
            &extra,
        ) {
            Ok(id) => {
                if self.options.audit_enabled && !labels.is_empty() {
                    labels.sort_unstable();
                    labels.dedup();
                    let _ = self.store.lock().map(|conn| {
                        crate::cli_write::audit(
                            &conn,
                            "redaction",
                            &id,
                            &scope,
                            &input.session_id,
                            &claims.sub,
                            &labels.join(","),
                        );
                    });
                }
                wire::raw_json(200, json!({"id":id,"status":"success"}).to_string())
            }
            Err(crate::StoreError::Invalid(message))
                if message.contains("requires the delegated Go pipeline") =>
            {
                wire::unsupported()
            }
            Err(_) => wire::error(500, "INTERNAL_ERROR", "Failed to save memory"),
        }
    }

    /// The write-path conflict check (`conflict.Checker.Check`) reduced to
    /// its "nothing to litigate" outcome. `Some(true)`: no duplicate and no
    /// candidate at or above the contradiction threshold; `Some(false)`: a
    /// repeat or contradiction band exists (needs the unported resolver);
    /// `None`: the check itself failed, which the shipped service treats as
    /// "store unchanged".
    fn conflict_free(
        &self,
        content: &str,
        scope: &str,
        embedding: &crate::GeneratedEmbedding,
    ) -> Option<bool> {
        let conn = self.store.lock().ok()?;
        let hash = crate::write::content_hash(content);
        let duplicate: Option<String> = conn
            .query_row(
                "SELECT id FROM memories WHERE content_hash = ? AND scope = ? AND consolidation_status != 'archived' AND review_status = 'approved' AND retired_at IS NULL AND (superseded_by IS NULL OR superseded_by = '') ORDER BY created_at DESC LIMIT 1",
                rusqlite::params![hash, scope],
                |row| row.get(0),
            )
            .optional()
            .ok()?;
        if duplicate.is_some() {
            return Some(false);
        }
        let threshold = self.options.conflict_threshold;
        let vector = embedding.vector.clone();
        // The shipped checker searches through the store, which records the
        // query in the retrieval statistics like any other search.
        let started = std::time::Instant::now();
        let outcome = crate::retrieval::search_checked(
            &conn,
            &embedding.vector,
            &embedding.source,
            scope,
            self.options.conflict_candidates,
            |row| {
                !row.superseded_by.is_empty()
                    || row.embedding.is_empty()
                    || cosine(&vector, &row.embedding) < threshold
            },
        );
        match outcome {
            Ok(hits) => {
                self.metrics.record(
                    hits.len(),
                    hits.first().map_or(0.0, |hit| f64::from(hit.score)),
                    started.elapsed(),
                );
                Some(true)
            }
            Err(crate::StoreError::Invalid(_)) => Some(false),
            Err(_) => {
                self.metrics.record(0, 0.0, started.elapsed());
                None
            }
        }
    }

    pub(super) fn delete(&self, params: &BTreeMap<String, String>) -> wire::Reply {
        let id = params.get("id").map_or("", String::as_str);
        if id.is_empty() {
            return wire::error(400, "INVALID_REQUEST", "missing required parameter: id");
        }
        if !self.options.direct_writes
            || !self
                .store
                .lock()
                .is_ok_and(|conn| crate::cli_delete_admission::checked(&conn, id).is_ok())
        {
            return wire::unsupported();
        }
        match self.store.delete_service(id, self.options.audit_enabled) {
            Ok(true) => wire::raw_json(200, "{\"deleted\":true}"),
            Ok(false) => wire::error(404, "NOT_FOUND", &format!("memory not found: {id}")),
            Err(_) => wire::error(500, "INTERNAL_ERROR", "failed to delete memory"),
        }
    }

    pub(super) fn search(&self, body: &[u8]) -> wire::Reply {
        let Some(input) = sync_decode::search_args(body) else {
            return wire::error_logged(
                400,
                "INVALID_REQUEST",
                "Bad request body",
                &go_json::unmarshal_error(body, &go_json::Shape::Search),
            );
        };
        if input.extended {
            return wire::unsupported();
        }
        let started = std::time::Instant::now();
        let embedding = self.embed(&input.query);
        let limit = if input.limit <= 0 {
            5
        } else {
            usize::try_from(input.limit).unwrap_or(usize::MAX)
        };
        let result = self.store.lock().and_then(|conn| {
            crate::retrieval::search_checked(
                &conn,
                &embedding.vector,
                &embedding.source,
                &input.scope,
                limit,
                |_| true,
            )
        });
        match result {
            Ok(mut hits) => {
                self.metrics.record(
                    hits.len(),
                    hits.first().map_or(0.0, |hit| f64::from(hit.score)),
                    started.elapsed(),
                );
                for hit in &mut hits {
                    hit.memory.redact_pii();
                }
                let rendered = hits
                    .iter()
                    .map(|hit| {
                        format!(
                            "{{\"memory\":{},\"similarity_score\":{}}}",
                            hit.memory.to_go_json_embedded(),
                            crate::gojson::number_f32(hit.score)
                        )
                    })
                    .collect::<Vec<_>>();
                wire::raw_json(200, read::array(&rendered))
            }
            Err(crate::StoreError::Invalid(_)) => wire::unsupported(),
            Err(_) => wire::error(500, "INTERNAL_ERROR", "Search failed"),
        }
    }

    pub(super) fn token_revoke(&self, body: &[u8]) -> wire::Reply {
        let Some((token, jti)) = sync_decode::revoke_args(body) else {
            return wire::error_logged(
                400,
                "INVALID_REQUEST",
                "Bad request body",
                &go_json::unmarshal_error(body, &go_json::Shape::Revoke),
            );
        };
        let input = Revoke { token, jti };
        let mut jti = input.jti.trim().to_owned();
        if jti.is_empty() && !input.token.trim().is_empty() {
            jti = match extract_jti(input.token.trim()) {
                Ok(jti) => jti,
                Err(reason) => {
                    return wire::error_logged(
                        400,
                        "INVALID_REQUEST",
                        "Invalid token value",
                        &reason,
                    );
                }
            };
        }
        if jti.is_empty() {
            return wire::error(
                400,
                "INVALID_REQUEST",
                "either 'token' or 'jti' is required",
            );
        }
        if !self.revoke(&jti) {
            return wire::error(
                500,
                "INTERNAL_ERROR",
                "token revoked in memory but persistence failed",
            );
        }
        wire::raw_json(200, json!({"status":"revoked","jti":jti}).to_string())
    }
}
