//! Byte-oriented JWT verification and store-owned profile/revocation boundaries.

use super::Server;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
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

    /// Signature, claim shape and in-memory revocation, in the shipped order;
    /// the error is the shipped diagnostic (logged, never returned to peers).
    pub fn verify(&self, token: &str) -> Result<Claims, &'static str> {
        let parts = token.split('.').collect::<Vec<_>>();
        let [header, payload, signature] = parts[..] else {
            return Err("invalid token structure");
        };
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.secret).map_err(|_| "invalid signature")?;
        mac.update(format!("{header}.{payload}").as_bytes());
        let signature_bytes = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| "invalid signature")?;
        if URL_SAFE_NO_PAD.encode(&signature_bytes) != signature {
            return Err("invalid signature");
        }
        mac.verify_slice(&signature_bytes)
            .map_err(|_| "invalid signature")?;
        let decoded = URL_SAFE_NO_PAD
            .decode(payload.trim_end_matches('='))
            .map_err(|_| "illegal base64 data")?;
        let claims: Claims = serde_json::from_slice(&decoded).map_err(|_| "invalid payload")?;
        if claims.sub.trim().is_empty() {
            return Err("token has empty subject");
        }
        if claims.iss != "symaira-memory" {
            return Err("invalid issuer");
        }
        if self
            .revoked
            .lock()
            .map_err(|_| "token has been revoked")?
            .contains(&claims.jti)
        {
            return Err("token has been revoked");
        }
        Ok(claims)
    }

    pub fn revoke_in_memory(&self, jti: &str) -> bool {
        self.revoked.lock().is_ok_and(|mut revoked| {
            revoked.insert(jti.into());
            true
        })
    }
}

impl Server {
    pub(super) fn authenticate(&self, header: &str) -> Result<Claims, &'static str> {
        let token = header
            .strip_prefix("Bearer ")
            .ok_or("invalid token structure")?;
        let claims = self.auth.verify(token)?;
        let conn = self.store.lock().map_err(|_| "token has been revoked")?;
        // The existing Go provider ignores persistent lookup errors. No open
        // access is introduced; signature, expiry and in-memory denial still apply.
        let revoked: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM jwt_revocations WHERE jti=?",
                [&claims.jti],
                |row| row.get(0),
            )
            .unwrap_or(false);
        if revoked {
            return Err("token has been revoked");
        }
        if chrono::Utc::now().timestamp() > claims.exp {
            return Err("token expired");
        }
        Ok(claims)
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
            None => {
                super::log::warn(
                    "JWT subject has no matching profile; granting default access",
                    &[("subject", claims.sub.clone())],
                );
                Ok(())
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
