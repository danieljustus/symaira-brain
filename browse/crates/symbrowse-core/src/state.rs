#![deny(unsafe_code)]

//! Backward-compatible browser-state file codec.

use std::{fmt, path::Path};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, Generate, KeyInit, Nonce as AeadNonce, Payload},
};
mod model;
use model::Header;
pub(crate) use model::StateHeader;
pub use model::{Cookie, OriginState, State};

pub const SCHEMA_VERSION: u32 = 3;
pub const FILE_MAGIC: &[u8] = b"SYMBROWSE-STATE\0";
const NONCE_SIZE: usize = 12;
const MAX_ENCRYPTED_PLAINTEXT_BYTES: usize = 64 << 20;

#[derive(Debug, Eq, PartialEq)]
pub enum StateError {
    InvalidName(&'static str),
    InvalidFile,
    InvalidHeader(String),
    InvalidPayload(String),
    KeyRequired,
    InvalidKeyLength(usize),
    InvalidKeySource,
    Truncated,
    Decrypt,
    HeaderAuthentication(Box<StateError>),
    MetadataMismatch,
    PlaintextTooLarge,
}

impl fmt::Display for StateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName(message) => formatter.write_str(message),
            Self::InvalidFile => formatter.write_str("file is not a symbrowse state file"),
            Self::InvalidHeader(error) => write!(formatter, "parse state header: {error}"),
            Self::InvalidPayload(error) => write!(formatter, "parse state payload: {error}"),
            Self::KeyRequired => formatter.write_str("encrypted state requires a key provider"),
            Self::InvalidKeyLength(length) => {
                write!(formatter, "encryption key must be 32 bytes, got {length}")
            }
            Self::InvalidKeySource => formatter.write_str("encrypted state key source is required"),
            Self::Truncated => formatter.write_str("encrypted state file is truncated"),
            Self::Decrypt => formatter.write_str("decrypt state file: aead::Error"),
            Self::HeaderAuthentication(error) => {
                write!(formatter, "authenticate state header: {error}")
            }
            Self::MetadataMismatch => formatter
                .write_str("decrypt state file: plaintext state metadata does not match header"),
            Self::PlaintextTooLarge => write!(
                formatter,
                "plaintext exceeds maximum size of {MAX_ENCRYPTED_PLAINTEXT_BYTES} bytes"
            ),
        }
    }
}

impl std::error::Error for StateError {}

pub fn validate_name(name: &str) -> Result<(), StateError> {
    if name.is_empty() {
        return Err(StateError::InvalidName("state name is required"));
    }
    if name.len() > 128 {
        return Err(StateError::InvalidName("state name is too long"));
    }
    if Path::new(name).file_name().and_then(|value| value.to_str()) != Some(name) {
        return Err(StateError::InvalidName(
            "state name must not contain path separators",
        ));
    }
    if name
        .chars()
        .any(|character| character < '\u{20}' || character == '\u{7f}')
    {
        return Err(StateError::InvalidName(
            "state name contains control characters",
        ));
    }
    Ok(())
}

pub fn decode(raw: &[u8], key: Option<&[u8]>) -> Result<State, StateError> {
    let data = raw
        .strip_prefix(FILE_MAGIC)
        .ok_or(StateError::InvalidFile)?;
    if let Some(newline) = data.iter().position(|byte| *byte == b'\n')
        && let Ok(header) = serde_json::from_slice::<Header>(&data[..newline])
        && header.schema_version >= 2
    {
        return decode_versioned(&header, &data[..newline], &data[newline + 1..], key);
    }
    decode_legacy(data, key)
}

pub(crate) fn encode_current(state: &State, key: Option<&[u8]>) -> Result<Vec<u8>, StateError> {
    let header = Header {
        schema_version: state.schema_version,
        saved_at: state.saved_at.clone(),
        expires_at: state.expires_at.clone(),
        key_source: state.key_source.clone(),
    };
    let header = serde_json::to_vec(&header)
        .map_err(|error| StateError::InvalidHeader(error.to_string()))?;
    let payload =
        serde_json::to_vec(state).map_err(|error| StateError::InvalidPayload(error.to_string()))?;
    let body = match key {
        Some(key) => encrypt(&payload, &header, key)?,
        None => payload,
    };
    let mut raw = FILE_MAGIC.to_vec();
    raw.extend(header);
    raw.push(b'\n');
    raw.extend(body);
    Ok(raw)
}

pub(crate) fn read_header(raw: &[u8], key: Option<&[u8]>) -> Result<StateHeader, StateError> {
    let data = raw
        .strip_prefix(FILE_MAGIC)
        .ok_or(StateError::InvalidFile)?;
    if let Some(newline) = data.iter().position(|byte| *byte == b'\n')
        && let Ok(header) = serde_json::from_slice::<Header>(&data[..newline])
        && header.schema_version >= 2
    {
        if header.schema_version >= 3 {
            if let Some(key) = key {
                // Destructive timestamp decisions cannot trust key_source:
                // it is itself unauthenticated until the AAD is verified.
                let _ = decrypt(&data[newline + 1..], &data[..newline], key)
                    .map_err(|error| StateError::HeaderAuthentication(Box::new(error)))?;
            } else if !header.key_source.is_empty() && header.key_source != "none" {
                return Err(StateError::KeyRequired);
            }
        }
        return Ok(StateHeader {
            saved_at: header.saved_at,
            expires_at: header.expires_at,
        });
    }
    let state = decode_legacy(data, key)?;
    Ok(StateHeader {
        saved_at: state.saved_at,
        expires_at: state.expires_at,
    })
}

fn decode_versioned(
    header: &Header,
    header_bytes: &[u8],
    body: &[u8],
    key: Option<&[u8]>,
) -> Result<State, StateError> {
    let encrypted = !header.key_source.is_empty() && header.key_source != "none";
    let payload = if encrypted {
        let key = key.ok_or(StateError::KeyRequired)?;
        let aad = if header.schema_version >= 3 {
            header_bytes
        } else {
            &[]
        };
        decrypt(body, aad, key)?
    } else {
        body.to_vec()
    };
    let mut state: State = serde_json::from_slice(&payload)
        .map_err(|error| StateError::InvalidPayload(error.to_string()))?;
    if header.schema_version >= 3
        && header.key_source == "none"
        && (state.key_source != header.key_source
            || state.saved_at != header.saved_at
            || state.expires_at != header.expires_at)
    {
        return Err(StateError::MetadataMismatch);
    }
    state.schema_version = header.schema_version;
    state.saved_at.clone_from(&header.saved_at);
    state.expires_at.clone_from(&header.expires_at);
    state.key_source.clone_from(&header.key_source);
    Ok(state)
}

fn decode_legacy(data: &[u8], key: Option<&[u8]>) -> Result<State, StateError> {
    if let Some(key) = key {
        if let Ok(payload) = decrypt(data, &[], key)
            && let Ok(mut state) = serde_json::from_slice::<State>(&payload)
        {
            if state.schema_version == 0 {
                state.schema_version = 1;
            }
            return Ok(state);
        }
        if let Ok(mut state) = serde_json::from_slice::<State>(data)
            && matches!(state.schema_version, 0 | 1)
        {
            if state.schema_version == 0 {
                state.schema_version = 1;
            }
            return Ok(state);
        }
        return decrypt(data, &[], key).and_then(parse_legacy_payload);
    }
    parse_legacy_payload(data.to_vec())
}

fn parse_legacy_payload(payload: Vec<u8>) -> Result<State, StateError> {
    let mut state: State = serde_json::from_slice(&payload)
        .map_err(|error| StateError::InvalidPayload(error.to_string()))?;
    if state.schema_version == 0 {
        state.schema_version = 1;
    }
    Ok(state)
}

pub fn decrypt(body: &[u8], aad: &[u8], key: &[u8]) -> Result<Vec<u8>, StateError> {
    if key.len() != 32 {
        return Err(StateError::InvalidKeyLength(key.len()));
    }
    if body.len() < NONCE_SIZE {
        return Err(StateError::Truncated);
    }
    if body.len() > NONCE_SIZE + MAX_ENCRYPTED_PLAINTEXT_BYTES + 16 {
        return Err(StateError::PlaintextTooLarge);
    }
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|_| StateError::InvalidKeyLength(key.len()))?;
    let (nonce, ciphertext) = body.split_at(NONCE_SIZE);
    let nonce = Nonce::try_from(nonce).map_err(|_| StateError::Truncated)?;
    cipher
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| StateError::Decrypt)
}

pub fn encrypt(plaintext: &[u8], aad: &[u8], key: &[u8]) -> Result<Vec<u8>, StateError> {
    let nonce = AeadNonce::<Aes256Gcm>::generate();
    encrypt_with_nonce(plaintext, aad, key, nonce.as_ref())
}

fn encrypt_with_nonce(
    plaintext: &[u8],
    aad: &[u8],
    key: &[u8],
    nonce: &[u8],
) -> Result<Vec<u8>, StateError> {
    if key.len() != 32 {
        return Err(StateError::InvalidKeyLength(key.len()));
    }
    if nonce.len() != NONCE_SIZE {
        return Err(StateError::Truncated);
    }
    if plaintext.len() > MAX_ENCRYPTED_PLAINTEXT_BYTES {
        return Err(StateError::PlaintextTooLarge);
    }
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|_| StateError::InvalidKeyLength(key.len()))?;
    let mut output = nonce.to_vec();
    let nonce = Nonce::try_from(nonce).map_err(|_| StateError::Truncated)?;
    output.extend(
        cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| StateError::Decrypt)?,
    );
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{
        FILE_MAGIC, MAX_ENCRYPTED_PLAINTEXT_BYTES, NONCE_SIZE, StateError, decode, decrypt,
        encrypt_with_nonce, validate_name,
    };

    #[test]
    fn validates_names_like_go() {
        for invalid in ["", "../escape", "a/b", "a\0b"] {
            assert!(validate_name(invalid).is_err(), "{invalid:?}");
        }
        assert_eq!(
            validate_name(&"x".repeat(129)),
            Err(StateError::InvalidName("state name is too long"))
        );
        for valid in ["demo", "my-state_1", "A.B-c"] {
            validate_name(valid).unwrap();
        }
    }

    #[test]
    fn fixed_nonce_vectors_match_go_bytes() {
        let key = [0xab; 32];
        for (version, raw) in [
            (
                1_u8,
                include_bytes!("../../../testdata/port/state/encrypted-v1.state").as_slice(),
            ),
            (
                2,
                include_bytes!("../../../testdata/port/state/encrypted-v2.state").as_slice(),
            ),
            (
                3,
                include_bytes!("../../../testdata/port/state/encrypted-v3.state").as_slice(),
            ),
        ] {
            let data = raw.strip_prefix(FILE_MAGIC).unwrap();
            let (header, body) = if version >= 2 {
                let newline = data.iter().position(|byte| *byte == b'\n').unwrap();
                (&data[..newline], &data[newline + 1..])
            } else {
                (&[][..], data)
            };
            let state = decode(raw, Some(&key)).unwrap();
            let payload = serde_json::to_vec(&state).unwrap();
            let aad = if version >= 3 { header } else { &[] };
            assert_eq!(
                encrypt_with_nonce(&payload, aad, &key, &[version; 12]).unwrap(),
                body
            );
        }
    }

    #[test]
    fn decrypt_rejects_oversized_ciphertext_before_authentication() {
        let oversized = vec![0; NONCE_SIZE + MAX_ENCRYPTED_PLAINTEXT_BYTES + 17];
        assert_eq!(
            decrypt(&oversized, &[], &[0xab; 32]),
            Err(StateError::PlaintextTooLarge)
        );
    }
}
