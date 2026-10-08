//! HTTP mutations retain service provenance, profile authorization and Store ownership.

use super::{Server, auth::Claims, read, wire};
use crate::DirectWrite;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Default, Deserialize)]
struct Set {
    #[serde(default)]
    content: String,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    entities: Vec<String>,
    #[serde(default)]
    working: bool,
}

#[derive(Default, Deserialize)]
struct Search {
    #[serde(default)]
    query: String,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    limit: i64,
    #[serde(flatten)]
    remaining: BTreeMap<String, serde_json::Value>,
}

impl Server {
    pub(super) fn set(&self, body: &[u8], claims: &Claims) -> wire::Reply {
        let Ok(input) = serde_json::from_slice::<Set>(body) else {
            return wire::error(400, "INVALID_REQUEST", "Bad request body");
        };
        if !self.options.direct_writes
            || input.working
            || !input.session_id.is_empty()
            || !input.entities.is_empty()
        {
            return wire::unsupported();
        }
        if !crate::direct_content_supported(&input.content)
            || input
                .metadata
                .values()
                .any(|value| !crate::direct_text_supported(value))
        {
            return wire::unsupported();
        }
        let request = DirectWrite {
            content: input.content,
            scope: input.scope,
            metadata: input.metadata,
            author: claims.sub.clone(),
            kind: String::new(),
            entities: Vec::new(),
            staged: false,
            quantize_binary: false,
            conflict_enabled: false,
        };
        match self.store.set_direct_service(
            &request,
            &self.generator,
            "http",
            None,
            self.options.audit_enabled,
        ) {
            Ok(id) => wire::raw_json(200, json!({"id":id,"status":"success"}).to_string()),
            Err(_) => wire::error(500, "INTERNAL_ERROR", "Failed to save memory"),
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
        let Ok(input) = serde_json::from_slice::<Search>(body) else {
            return wire::error(400, "INVALID_REQUEST", "Bad request body");
        };
        if !crate::direct_text_supported(&input.query)
            || input.remaining.values().any(|value| {
                !matches!(
                    value,
                    serde_json::Value::Null | serde_json::Value::Bool(false)
                ) && value.as_str() != Some("")
                    && value.as_i64() != Some(0)
            })
        {
            return wire::unsupported();
        }
        let embedding = self.generator.generate(&input.query);
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
                read::safe_row,
            )
        });
        match result {
            Ok(hits) => {
                let rendered = hits
                    .iter()
                    .map(|hit| {
                        format!(
                            "{{\"memory\":{},\"similarity_score\":{}}}",
                            read::full_json(&hit.memory),
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
        #[derive(Deserialize)]
        struct Revoke {
            #[serde(default)]
            jti: String,
            #[serde(default)]
            token: String,
        }
        let Ok(input) = serde_json::from_slice::<Revoke>(body) else {
            return wire::error(400, "INVALID_REQUEST", "Bad request body");
        };
        let mut jti = input.jti.trim().to_owned();
        if jti.is_empty() && !input.token.trim().is_empty() {
            let parts = input.token.trim().split('.').collect::<Vec<_>>();
            let payload = (parts.len() == 3).then(|| parts[1]);
            let claims = payload
                .and_then(|p| URL_SAFE_NO_PAD.decode(p).ok())
                .and_then(|p| serde_json::from_slice::<super::auth::Claims>(&p).ok());
            let Some(claims) = claims.filter(|c| !c.jti.is_empty()) else {
                return wire::error(400, "INVALID_REQUEST", "Invalid token value");
            };
            jti = claims.jti;
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
