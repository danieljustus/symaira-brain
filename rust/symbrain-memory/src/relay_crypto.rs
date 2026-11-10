//! The encrypted-relay payload cipher of `security.CryptoEngine`:
//! AES-256-GCM under a PBKDF2-HMAC-SHA256 key (600 000 iterations).
//!
//! Layout `[0x01][salt 16][nonce 12][ciphertext||tag]`; payloads without the
//! version byte use the legacy `[salt 8][nonce 12][ciphertext||tag]`. Like the
//! shipped engine, one instance derives the key for a passphrase once and
//! reuses it (and, for encryption, its salt) until the passphrase or the salt
//! of a payload to decrypt changes.

use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::pbkdf2::{PBKDF2_HMAC_SHA256, derive};
use ring::rand::{SecureRandom, SystemRandom, generate};
use std::num::NonZeroU32;

const ITERATIONS: u32 = 600_000;
const VERSION_V1: u8 = 0x01;
const SALT_LEGACY: usize = 8;
const SALT_V1: usize = 16;
const MIN_CIPHERTEXT: usize = 16;

struct Derived {
    passphrase: Vec<u8>,
    salt: Vec<u8>,
    key: [u8; 32],
}

/// A cipher with the shipped key cache.
#[derive(Default)]
pub struct RelayCipher {
    cached: Option<Derived>,
}

fn derive_key(passphrase: &[u8], salt: &[u8]) -> [u8; 32] {
    let mut key = [0_u8; 32];
    if let Some(iterations) = NonZeroU32::new(ITERATIONS) {
        derive(PBKDF2_HMAC_SHA256, iterations, salt, passphrase, &mut key);
    }
    key
}

/// A fresh v1 salt taken straight from the random source, so no placeholder
/// value is ever staged in a buffer that later feeds key derivation.
fn random_salt(random: &SystemRandom) -> Result<[u8; SALT_V1], String> {
    generate::<[u8; SALT_V1]>(random)
        .map(ring::rand::Random::expose)
        .map_err(|_| "random source failed".to_owned())
}

impl RelayCipher {
    /// A cipher with an empty key cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn key_for(&mut self, passphrase: &[u8], salt: &[u8]) -> [u8; 32] {
        if let Some(cached) = &self.cached
            && cached.passphrase == passphrase
            && cached.salt == salt
        {
            return cached.key;
        }
        let key = derive_key(passphrase, salt);
        self.cached = Some(Derived {
            passphrase: passphrase.to_vec(),
            salt: salt.to_vec(),
            key,
        });
        key
    }

    /// `Encrypt`: version byte, salt, nonce, sealed payload.
    ///
    /// # Errors
    /// The random source or the cipher failed.
    pub fn encrypt(&mut self, plaintext: &[u8], passphrase: &[u8]) -> Result<Vec<u8>, String> {
        let random = SystemRandom::new();
        let (salt, key) = match &self.cached {
            Some(cached) if cached.passphrase == passphrase => (cached.salt.clone(), cached.key),
            _ => {
                let salt = random_salt(&random)?;
                let key = derive_key(passphrase, &salt);
                self.cached = Some(Derived {
                    passphrase: passphrase.to_vec(),
                    salt: salt.to_vec(),
                    key,
                });
                (salt.to_vec(), key)
            }
        };
        let unbound =
            UnboundKey::new(&AES_256_GCM, &key).map_err(|_| "cipher setup failed".to_owned())?;
        let sealing = LessSafeKey::new(unbound);
        let mut nonce_bytes = [0_u8; NONCE_LEN];
        random
            .fill(&mut nonce_bytes)
            .map_err(|_| "random source failed".to_owned())?;
        let mut sealed = plaintext.to_vec();
        sealing
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce_bytes),
                Aad::empty(),
                &mut sealed,
            )
            .map_err(|_| "seal failed".to_owned())?;
        let mut payload = Vec::with_capacity(1 + salt.len() + NONCE_LEN + sealed.len());
        payload.push(VERSION_V1);
        payload.extend_from_slice(&salt);
        payload.extend_from_slice(&nonce_bytes);
        payload.extend_from_slice(&sealed);
        Ok(payload)
    }

    fn open(
        &mut self,
        passphrase: &[u8],
        salt: &[u8],
        nonce: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, String> {
        let failure = || "decryption failed: invalid passphrase or corrupted payload".to_owned();
        let key = self.key_for(passphrase, salt);
        let unbound = UnboundKey::new(&AES_256_GCM, &key).map_err(|_| failure())?;
        let opening = LessSafeKey::new(unbound);
        let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| failure())?;
        let mut buffer = ciphertext.to_vec();
        let plain = opening
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::empty(),
                &mut buffer,
            )
            .map_err(|_| failure())?;
        Ok(plain.to_vec())
    }

    /// `Decrypt`: the v1 layout first, then the legacy one.
    ///
    /// # Errors
    /// Go's `encrypted payload too short` / `decryption failed: ...` texts.
    pub fn decrypt(&mut self, payload: &[u8], passphrase: &[u8]) -> Result<Vec<u8>, String> {
        let too_short = || "encrypted payload too short".to_owned();
        if payload.is_empty() {
            return Err(too_short());
        }
        if payload[0] == VERSION_V1 && payload.len() >= 1 + SALT_V1 + NONCE_LEN + MIN_CIPHERTEXT {
            let salt = &payload[1..=SALT_V1];
            let nonce = &payload[1 + SALT_V1..1 + SALT_V1 + NONCE_LEN];
            let ciphertext = &payload[1 + SALT_V1 + NONCE_LEN..];
            if let Ok(plain) = self.open(passphrase, salt, nonce, ciphertext) {
                return Ok(plain);
            }
        }
        if payload.len() < SALT_LEGACY + NONCE_LEN + MIN_CIPHERTEXT {
            return Err(too_short());
        }
        let salt = &payload[..SALT_LEGACY];
        let nonce = &payload[SALT_LEGACY..SALT_LEGACY + NONCE_LEN];
        let ciphertext = &payload[SALT_LEGACY + NONCE_LEN..];
        self.open(passphrase, salt, nonce, ciphertext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_reuses_the_salt_per_engine() {
        let mut cipher = RelayCipher::new();
        let first = cipher.encrypt(b"one", b"pw").unwrap();
        let second = cipher.encrypt(b"two", b"pw").unwrap();
        assert_eq!(first[0], VERSION_V1);
        assert_eq!(first[1..17], second[1..17], "one salt per engine run");
        assert_ne!(first[17..29], second[17..29], "a fresh nonce per payload");
        let mut reader = RelayCipher::new();
        assert_eq!(reader.decrypt(&first, b"pw").unwrap(), b"one");
        assert_eq!(reader.decrypt(&second, b"pw").unwrap(), b"two");
        assert_eq!(
            reader.decrypt(&first, b"other").unwrap_err(),
            "decryption failed: invalid passphrase or corrupted payload"
        );
        assert_eq!(
            reader.decrypt(b"", b"pw").unwrap_err(),
            "encrypted payload too short"
        );
        assert_eq!(
            reader.decrypt(&[1; 20], b"pw").unwrap_err(),
            "encrypted payload too short"
        );
    }

    #[test]
    fn random_salts_are_full_length_and_distinct() {
        let random = SystemRandom::new();
        let first = random_salt(&random).unwrap();
        let second = random_salt(&random).unwrap();
        assert_eq!(first.len(), SALT_V1);
        assert_ne!(first, second, "two draws must not repeat");
        assert_ne!(first, [0_u8; SALT_V1], "the salt is never the zero buffer");
    }

    #[test]
    fn legacy_payloads_decrypt() {
        let salt = [7_u8; SALT_LEGACY];
        let key = derive_key(b"pw", &salt);
        let sealing = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &key).unwrap());
        let nonce = [3_u8; NONCE_LEN];
        let mut body = b"legacy".to_vec();
        sealing
            .seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::empty(), &mut body)
            .unwrap();
        let mut payload = salt.to_vec();
        payload.extend_from_slice(&nonce);
        payload.extend_from_slice(&body);
        assert_eq!(
            RelayCipher::new().decrypt(&payload, b"pw").unwrap(),
            b"legacy"
        );
    }
}
