//! HTTP mutations retain service provenance, profile authorization and Store
//! ownership. Set, delete and search run the same native pipelines as the
//! CLI (`MemoryService.Set`, `Delete`, `Search`): redaction, conflict check,
//! entity linking, secondary facts and every stored-row diagnostic.

use super::{Server, auth::Claims, go_json, read, sync_decode, wire};
use crate::{CliSet, Origin};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::json;
use std::collections::BTreeMap;

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
        let author = if claims.sub.is_empty() {
            "api".to_owned()
        } else {
            claims.sub.clone()
        };
        let request = CliSet {
            content: input.content.clone().into_bytes(),
            scope: input.scope.clone().into_bytes(),
            kind: String::new(),
            metadata: input.metadata.clone(),
            author: author.into_bytes(),
            entities: input
                .entities
                .iter()
                .map(|name| name.clone().into_bytes())
                .collect(),
            staged: false,
            quantize_binary: self.options.quantize_binary,
            prefilter: self.options.prefilter,
            conflict: self.options.conflict.clone(),
        };
        let embed = |text: &[u8]| self.embed(&String::from_utf8_lossy(text));
        let observe = |results: usize, top: f64, latency: std::time::Duration| {
            self.metrics.record(results, top, latency);
        };
        let origin = Origin {
            source_tool: "http",
            session: &input.session_id,
            working_ttl: input.working.then(|| {
                chrono::Duration::from_std(self.options.working_ttl)
                    .unwrap_or_else(|_| chrono::Duration::hours(24))
            }),
            governed: false,
            audit_enabled: self.options.audit_enabled,
            embed: &embed,
            model: self.generator.model(),
            observe: Some(&observe),
        };
        let mut stderr = std::io::stderr();
        let mut log = crate::LogSink(&mut stderr);
        match self.store.set_service(&request, &origin, &mut log) {
            Ok(id) => wire::raw_json(200, json!({"id":id,"status":"success"}).to_string()),
            Err(error) => wire::error_logged(
                500,
                "INTERNAL_ERROR",
                "Failed to save memory",
                &error.to_string(),
            ),
        }
    }

    pub(super) fn delete(&self, params: &BTreeMap<String, String>) -> wire::Reply {
        let id = params.get("id").map_or("", String::as_str);
        if id.is_empty() {
            return wire::error(400, "INVALID_REQUEST", "missing required parameter: id");
        }
        match self.store.delete_service(id, self.options.audit_enabled) {
            Ok(true) => wire::raw_json(200, "{\"deleted\":true}"),
            Ok(false) => wire::error(404, "NOT_FOUND", &format!("memory not found: {id}")),
            Err(error) => wire::error_logged(
                500,
                "INTERNAL_ERROR",
                "failed to delete memory",
                &error.to_string(),
            ),
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
        let limit = if input.limit <= 0 {
            5
        } else {
            usize::try_from(input.limit).unwrap_or(usize::MAX)
        };
        let mut filters = crate::SearchFilters {
            entity_id: String::new(),
            trust: crate::TrustFilter {
                min_confidence: input.min_confidence.clone(),
                verification_status: input.verification.clone(),
                exclude_superseded: input.exclude_superseded,
                max_age: None,
            },
            policy: crate::PolicyFilter {
                max_sensitivity: input.max_sensitivity.clone(),
                min_sharing_level: input.min_sharing_level.clone(),
                client_id: input.client_id.clone(),
            },
        };
        if !input.max_age.is_empty() {
            match crate::goduration::parse_max_age(&input.max_age) {
                Ok(nanos) => {
                    filters.trust.max_age = u64::try_from(nanos)
                        .ok()
                        .map(std::time::Duration::from_nanos);
                }
                Err(reason) => {
                    return wire::error_logged(
                        400,
                        "INVALID_REQUEST",
                        "invalid max_age parameter",
                        &reason,
                    );
                }
            }
        }
        if !input.entity.is_empty() {
            match self.store.resolve_entity_id(&input.entity) {
                Ok(Some(id)) => filters.entity_id = id,
                Ok(None) => {
                    return wire::error(
                        404,
                        "NOT_FOUND",
                        &format!("entity not found: {}", input.entity),
                    );
                }
                Err(error) => {
                    return wire::error_logged(
                        500,
                        "INTERNAL_ERROR",
                        "Search failed",
                        &format!("resolve entity: {error}"),
                    );
                }
            }
        }
        let embedding = self.embed(&input.query);
        let observe = |results: usize, top: f64, latency: std::time::Duration| {
            self.metrics.record(results, top, latency);
        };
        let result = self.store.search_filtered(&crate::ServiceSearch {
            vector: &embedding.vector,
            source: &embedding.source,
            scope: &input.scope,
            limit,
            prefilter: self.options.prefilter,
            filters: &filters,
            observe: Some(&observe),
        });
        match result {
            Ok(mut hits) => {
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
            Err(error) => {
                wire::error_logged(500, "INTERNAL_ERROR", "Search failed", &error.to_string())
            }
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
