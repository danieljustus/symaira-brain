//! Pure purpose-bound token primitives. Key storage, entropy, and the issuance
//! clock belong to the caller; Go remains the issuance and persistence oracle.

use base64::Engine;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::Sha256;
use std::fmt;

use crate::capability_wire::{base64_engine, canonical_json, decode_base64, decode_token};

pub const KEY_SIZE: usize = 32;
pub const SCOPE_ALL: &str = "*";
const DERIVE_INFO: &[u8] = b"symguard:capability:token-signing:v1";

/// Management operations are unreachable even through an explicit scope entry.
pub const CONTROL_PLANE_PREFIXES: [&str; 7] = [
    "symguard:config:",
    "symguard:grant:",
    "symguard:policy:",
    "symguard:token:",
    "symguard:audit:",
    "symguard:identity:",
    "symguard:capability:",
];

/// Field order and nil-versus-empty scope are part of the signed Go wire format.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Claims {
    #[serde(rename = "sub")]
    pub subject: String,
    pub purpose: String,
    pub scope: Option<Vec<String>>,
    pub iat: i64,
    pub exp: i64,
    pub jti: String,
}

/// Decoding alone does not establish authenticity or validate claims.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Token {
    pub claims: Claims,
    #[serde(rename = "sig")]
    pub signature: String,
}

/// The five sentinel classifications exposed by Go's capability package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    NoKeyMaterial,
    Malformed,
    InvalidSignature,
    Expired,
    InvalidClaims,
}

impl ErrorKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoKeyMaterial => "no_key_material",
            Self::Malformed => "malformed",
            Self::InvalidSignature => "invalid_signature",
            Self::Expired => "expired",
            Self::InvalidClaims => "invalid_claims",
        }
    }

    const fn message(self) -> &'static str {
        match self {
            Self::NoKeyMaterial => "capability: no key material",
            Self::Malformed => "capability: malformed token",
            Self::InvalidSignature => "capability: invalid signature",
            Self::Expired => "capability: token expired",
            Self::InvalidClaims => "capability: invalid claims",
        }
    }
}

/// Retains both sentinel identity and Go's wrapped diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    message: String,
}

impl Error {
    pub(crate) fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            message: kind.message().into(),
        }
    }

    pub(crate) fn decode() -> Self {
        Self {
            kind: ErrorKind::Malformed,
            message: "capability: decode: capability: malformed token".into(),
        }
    }

    fn claims(reason: &str) -> Self {
        Self {
            kind: ErrorKind::InvalidClaims,
            message: format!("capability: capability: invalid claims: {reason}"),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl Token {
    /// Encodes canonical Go JSON as unpadded URL-safe base64.
    #[must_use]
    pub fn encode(&self) -> String {
        base64_engine().encode(canonical_json(self))
    }

    /// Parses an untrusted token; use `verify_at` before trusting its claims.
    ///
    /// # Errors
    /// Returns `Malformed` for invalid base64, JSON, or field types.
    pub fn decode(encoded: &str) -> Result<Self, Error> {
        if encoded.is_empty() {
            return Err(Error::new(ErrorKind::Malformed));
        }
        decode_token(&decode_base64(encoded).map_err(|_| Error::decode())?)
            .map_err(|_| Error::decode())
    }
}

/// Derives the domain-separated signing key, never reusing the raw master.
///
/// # Errors
/// Returns `NoKeyMaterial` when the master is shorter than 32 bytes.
pub fn derive_key(master: &[u8]) -> Result<[u8; KEY_SIZE], Error> {
    if master.len() < KEY_SIZE {
        return Err(Error {
            kind: ErrorKind::NoKeyMaterial,
            message: format!(
                "capability: no key material: key material must be at least 32 bytes, got {}",
                master.len()
            ),
        });
    }
    let mut key = [0; KEY_SIZE];
    Hkdf::<Sha256>::new(None, master)
        .expand(DERIVE_INFO, &mut key)
        .map_err(|_| Error::new(ErrorKind::NoKeyMaterial))?;
    Ok(key)
}

fn mac(master: &[u8], claims: &Claims) -> Result<Hmac<Sha256>, Error> {
    let key = derive_key(master).map_err(|_| Error::new(ErrorKind::NoKeyMaterial))?;
    let mut mac =
        Hmac::<Sha256>::new_from_slice(&key).map_err(|_| Error::new(ErrorKind::NoKeyMaterial))?;
    mac.update(canonical_json(claims).as_bytes());
    Ok(mac)
}

/// Signs supplied claims, matching Go's pure `sign` primitive. This is not an
/// issuer: it does not choose a TTL/JTI or validate the supplied claims. Callers
/// must verify before using a token for an identity switch or policy decision.
///
/// # Errors
/// Returns `NoKeyMaterial` when signing is unavailable.
pub fn sign(master: &[u8], claims: Claims) -> Result<Token, Error> {
    let signature = base64_engine().encode(mac(master, &claims)?.finalize().into_bytes());
    Ok(Token { claims, signature })
}

/// Checks key, structure, signature, claims, then expiry, in that order.
/// `now` is trusted Unix seconds supplied by the caller. Like Go, a future iat
/// is allowed; equality with expiry is expired. Purpose is retained, not matched
/// against a requested operation by this primitive.
///
/// # Errors
/// Returns the first Go-compatible sentinel and diagnostic.
pub fn verify_at(master: &[u8], encoded: &str, now: i64) -> Result<Claims, Error> {
    if master.len() < KEY_SIZE {
        return Err(Error::new(ErrorKind::NoKeyMaterial));
    }
    let token = Token::decode(encoded)?;
    let signature =
        decode_base64(&token.signature).map_err(|_| Error::new(ErrorKind::InvalidSignature))?;
    mac(master, &token.claims)?
        .verify_slice(&signature)
        .map_err(|_| Error::new(ErrorKind::InvalidSignature))?;
    validate_claims(&token.claims)?;
    if token.claims.exp <= now {
        return Err(Error::new(ErrorKind::Expired));
    }
    Ok(token.claims)
}

/// Applies Go's structural invariants without checking expiry or authenticity.
///
/// # Errors
/// Returns `InvalidClaims` for the first failed invariant.
pub fn validate_claims(claims: &Claims) -> Result<(), Error> {
    if claims.subject.is_empty() || claims.purpose.is_empty() || claims.jti.is_empty() {
        return Err(Error::claims("subject, purpose, and jti must be non-empty"));
    }
    if claims.iat <= 0 {
        return Err(Error::claims("iat must be positive"));
    }
    if claims.exp <= claims.iat {
        return Err(Error::claims("exp must be after iat"));
    }
    let mut wildcard = false;
    for entry in claims.scope.as_deref().unwrap_or_default() {
        if entry.is_empty() {
            return Err(Error::claims("empty scope entry"));
        }
        if entry == SCOPE_ALL {
            if wildcard {
                return Err(Error::claims("duplicate wildcard scope"));
            }
            wildcard = true;
        }
    }
    Ok(())
}

#[must_use]
pub fn deny_control_plane(target: &str) -> bool {
    CONTROL_PLANE_PREFIXES
        .iter()
        .any(|prefix| target.starts_with(prefix))
}

/// Scope only narrows authority; combine this with the identity's policy ceiling.
#[must_use]
pub fn in_scope(scope: &[String], target: &str) -> bool {
    !target.is_empty()
        && !deny_control_plane(target)
        && scope
            .iter()
            .any(|entry| entry == SCOPE_ALL || entry == target)
}
