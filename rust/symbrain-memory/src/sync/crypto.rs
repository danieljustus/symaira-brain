//! Pinned RustCrypto implementation of the frozen relay format and key cache.

use super::{RelayCodec, RelayPayload, SyncError};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::Zeroizing;

const ITERATIONS: u32 = 600_000;
const AUTH_ERROR: &str = "decryption failed: invalid passphrase or corrupted payload";

/// Run-owned entropy source. Production uses the OS; owned fault probes can
/// inject deterministic bytes/errors without changing production Go.
pub trait EntropySource {
    /// # Errors
    /// Returns the source's entropy error without any secret bytes.
    fn fill(&mut self, bytes: &mut [u8]) -> Result<(), SyncError>;
}
struct OsEntropy;
impl EntropySource for OsEntropy {
    fn fill(&mut self, bytes: &mut [u8]) -> Result<(), SyncError> {
        getrandom::fill(bytes).map_err(|error| SyncError(error.to_string()))
    }
}
struct Cache {
    fingerprint: Zeroizing<[u8; 32]>,
    salt: Vec<u8>,
    key: Zeroizing<[u8; 32]>,
}

/// No durable key or passphrase storage. Cached keys/fingerprints are wiped
/// when rotated or dropped; Debug is intentionally absent.
pub struct CryptoEngine {
    entropy: Box<dyn EntropySource>,
    fingerprint_key: Option<Zeroizing<[u8; 32]>>,
    fingerprint_error: Option<String>,
    cache: Option<Cache>,
}
impl Default for CryptoEngine {
    fn default() -> Self {
        Self::with_entropy(Box::new(OsEntropy))
    }
}
impl CryptoEngine {
    #[must_use]
    pub fn with_entropy(entropy: Box<dyn EntropySource>) -> Self {
        Self {
            entropy,
            fingerprint_key: None,
            fingerprint_error: None,
            cache: None,
        }
    }
    fn fingerprint(&mut self, passphrase: &str) -> Result<Zeroizing<[u8; 32]>, SyncError> {
        if let Some(error) = &self.fingerprint_error {
            return Err(SyncError(format!("generate cache key: {error}")));
        }
        if self.fingerprint_key.is_none() {
            let mut key = Zeroizing::new([0; 32]);
            if let Err(error) = self.entropy.fill(key.as_mut()) {
                self.fingerprint_error = Some(error.to_string());
                return Err(SyncError(format!("generate cache key: {error}")));
            }
            self.fingerprint_key = Some(key);
        }
        let key = self
            .fingerprint_key
            .as_ref()
            .expect("initialized fingerprint key");
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key.as_ref())
            .expect("HMAC accepts fixed 32-byte key");
        mac.update(passphrase.as_bytes());
        let mut result = Zeroizing::new([0; 32]);
        result.copy_from_slice(&mac.finalize().into_bytes());
        Ok(result)
    }
    fn derive(passphrase: &str, salt: &[u8]) -> Zeroizing<[u8; 32]> {
        let mut key = Zeroizing::new([0; 32]);
        pbkdf2::pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), salt, ITERATIONS, key.as_mut());
        key
    }
    fn material(&mut self, passphrase: &str) -> Result<(Zeroizing<[u8; 32]>, Vec<u8>), SyncError> {
        let fingerprint = self.fingerprint(passphrase)?;
        if let Some(cache) = &self.cache {
            if cache.fingerprint[..] == fingerprint[..] {
                return Ok((Zeroizing::new(*cache.key), cache.salt.clone()));
            }
        }
        let mut salt = vec![0; 16];
        self.entropy.fill(&mut salt)?;
        let key = Self::derive(passphrase, &salt);
        self.cache = Some(Cache {
            fingerprint,
            salt: salt.clone(),
            key: Zeroizing::new(*key),
        });
        Ok((key, salt))
    }
    fn key_for(&mut self, passphrase: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>, SyncError> {
        // Go keyFor suppresses the entropy error and AES then rejects nil key.
        let fingerprint = self
            .fingerprint(passphrase)
            .map_err(|_| SyncError("crypto/aes: invalid key size 0".into()))?;
        if let Some(cache) = &self.cache {
            if cache.fingerprint[..] == fingerprint[..] && cache.salt == salt {
                return Ok(Zeroizing::new(*cache.key));
            }
        }
        let key = Self::derive(passphrase, salt);
        self.cache = Some(Cache {
            fingerprint,
            salt: salt.to_vec(),
            key: Zeroizing::new(*key),
        });
        Ok(key)
    }
    /// Encrypts raw bytes as version1/salt16/nonce12/ciphertext+tag16.
    /// # Errors
    /// Returns entropy, key or authenticated-encryption errors.
    pub fn encrypt_bytes(
        &mut self,
        plaintext: &[u8],
        passphrase: &str,
    ) -> Result<Vec<u8>, SyncError> {
        let (key, salt) = self.material(passphrase)?;
        let cipher = Aes256Gcm::new_from_slice(key.as_ref()).expect("fixed AES256 key");
        let mut nonce = [0_u8; 12];
        self.entropy.fill(&mut nonce)?;
        let nonce_value = Nonce::from(nonce);
        let ciphertext = cipher
            .encrypt(&nonce_value, plaintext)
            .map_err(|_| SyncError("AES-GCM encryption failed".into()))?;
        let mut result = Vec::with_capacity(29 + ciphertext.len());
        result.push(1);
        result.extend(salt);
        result.extend(nonce);
        result.extend(ciphertext);
        Ok(result)
    }
    /// Decrypts version1 and legacy salt8 formats, retaining legacy fallback
    /// when a version-looking payload fails authentication.
    /// # Errors
    /// Returns short payload, key or authenticated-decryption errors.
    pub fn decrypt_bytes(
        &mut self,
        payload: &[u8],
        passphrase: &str,
    ) -> Result<Vec<u8>, SyncError> {
        if payload.first() == Some(&1) && payload.len() >= 45 {
            if let Ok(plain) = self.open(
                passphrase,
                &payload[1..17],
                &payload[17..29],
                &payload[29..],
            ) {
                return Ok(plain);
            }
        }
        if payload.len() < 36 {
            return Err(SyncError("encrypted payload too short".into()));
        }
        self.open(passphrase, &payload[..8], &payload[8..20], &payload[20..])
    }
    fn open(
        &mut self,
        passphrase: &str,
        salt: &[u8],
        nonce: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, SyncError> {
        let key = self.key_for(passphrase, salt)?;
        let cipher = Aes256Gcm::new_from_slice(key.as_ref()).expect("fixed AES256 key");
        let array: [u8; 12] = nonce.try_into().expect("validated format nonce");
        cipher
            .decrypt(&Nonce::from(array), ciphertext)
            .map_err(|_| SyncError(AUTH_ERROR.into()))
    }
}
impl RelayCodec for CryptoEngine {
    fn reset_phase(&mut self) {
        self.cache = None;
        self.fingerprint_key = None;
        self.fingerprint_error = None;
    }
    fn encode(&self, payload: &RelayPayload) -> Result<Vec<u8>, SyncError> {
        super::wire::relay_payload(payload)
    }
    fn decode(&self, payload: &[u8]) -> Result<RelayPayload, SyncError> {
        super::json::relay_payload(payload)
    }
    fn encrypt(&mut self, payload: &[u8], passphrase: &str) -> Result<Vec<u8>, SyncError> {
        self.encrypt_bytes(payload, passphrase)
    }
    fn decrypt(&mut self, blob: &[u8], passphrase: &str) -> Result<Vec<u8>, SyncError> {
        self.decrypt_bytes(blob, passphrase)
    }
}
