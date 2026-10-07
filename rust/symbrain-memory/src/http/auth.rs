//! Byte-oriented JWT verification and store-owned profile/revocation boundaries.

use super::Server;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use sha2::Sha256;
use std::collections::HashSet;
use std::sync::Mutex;
use zeroize::Zeroizing;

pub(super) struct Auth {
    secret: Zeroizing<Vec<u8>>,
    revoked: Mutex<HashSet<String>>,
}

#[derive(Default, Deserialize)]
pub(super) struct Claims {
    #[serde(default)]
    pub jti: String,
    #[serde(default)]
    pub iss: String,
    #[serde(default)]
    pub sub: String,
    #[serde(default)]
    pub exp: i64,
    #[serde(default, rename = "iat")]
    _issued_at: i64,
}

impl Auth {
    pub fn new(secret: Vec<u8>) -> Self {
        Self {
            secret: Zeroizing::new(secret),
            revoked: Mutex::default(),
        }
    }

    pub fn verify(&self, token: &str) -> Option<Claims> {
        let mut parts = token.split('.');
        let (header, payload, signature) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() {
            return None;
        }
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.secret).ok()?;
        mac.update(format!("{header}.{payload}").as_bytes());
        let signature_bytes = URL_SAFE_NO_PAD.decode(signature).ok()?;
        if URL_SAFE_NO_PAD.encode(&signature_bytes) != signature {
            return None;
        }
        mac.verify_slice(&signature_bytes).ok()?;
        let claims: Claims = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
        if claims.sub.trim().is_empty()
            || claims.iss != "symaira-memory"
            || chrono::Utc::now().timestamp() > claims.exp
            || self.revoked.lock().ok()?.contains(&claims.jti)
        {
            return None;
        }
        Some(claims)
    }

    pub fn revoke_in_memory(&self, jti: &str) -> bool {
        self.revoked.lock().is_ok_and(|mut revoked| {
            revoked.insert(jti.into());
            true
        })
    }
}

impl Server {
    pub(super) fn authenticate(&self, header: &str) -> Option<Claims> {
        let claims = self.auth.verify(header.strip_prefix("Bearer ")?)?;
        let conn = self.store.lock().ok()?;
        // The existing Go provider ignores persistent lookup errors. No open
        // access is introduced; signature, expiry and in-memory denial still apply.
        let revoked: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM jwt_revocations WHERE jti=?)",
                [&claims.jti],
                |row| row.get(0),
            )
            .unwrap_or(false);
        (!revoked).then_some(claims)
    }

    pub(super) fn write_authorized(&self, claims: &Claims) -> Result<(), &'static str> {
        let conn = self
            .store
            .lock()
            .map_err(|_| "failed to verify permissions")?;
        let role: Option<String> = conn
            .query_row(
                "SELECT role FROM profiles WHERE name=?",
                [&claims.sub],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| "failed to verify permissions")?;
        match role.as_deref() {
            None if self.options.require_profile => {
                Err("insufficient permissions: no profile registered for subject")
            }
            Some(role) if !["readwrite", "admin"].contains(&role) => {
                Err("insufficient permissions: read-only profile")
            }
            _ => Ok(()),
        }
    }

    pub(super) fn revoke(&self, jti: &str) -> bool {
        if !self.auth.revoke_in_memory(jti) {
            return false;
        }
        self.store.lock().is_ok_and(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO jwt_revocations(jti) VALUES(?)",
                params![jti],
            )
            .is_ok()
        })
    }
}
