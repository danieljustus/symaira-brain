//! Frozen Memory rotation format: salt16, nonce12, AES256-GCM ciphertext/tag.
use std::{
    io::Write,
    path::{Path, PathBuf},
};

use chrono::{DateTime, FixedOffset, Utc};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use sha2::{Digest, Sha256};
use symbrain_core::{GoText, go_path};

use crate::{startup_fs, startup_rotation_json};
use startup_rotation_json::Records;

pub(super) struct Entry {
    pub secret: String,
    pub expires: DateTime<FixedOffset>,
}

const SHORT: &str = "encrypted payload too short";
const CORRUPT: &str =
    "fallback secrets decryption failed: invalid primary secret or corrupted data";

fn path(secret: &Path) -> PathBuf {
    let mut bytes = go_path::os_bytes(secret.as_os_str());
    // filepath.Ext consults only the final component and its final dot.
    if let Some(dot) = bytes.iter().rposition(|b| *b == b'.')
        && !bytes[dot..]
            .iter()
            .any(|b| *b == b'/' || (cfg!(windows) && *b == b'\\'))
    {
        bytes.truncate(dot);
    }
    bytes.extend(b".secrets");
    PathBuf::from(go_path::from_bytes(&bytes))
}

fn key(primary: &[u8], salt: &[u8]) -> LessSafeKey {
    let mut digest = Sha256::new();
    digest.update(primary);
    digest.update(salt);
    let digest = digest.finalize();
    LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &digest).expect("SHA256 key is32 bytes"))
}

fn decrypt(data: &[u8], primary: &[u8]) -> Result<Records, GoText> {
    if data.len() < 44 {
        return Err(SHORT.into());
    }
    let mut body = data[28..].to_vec();
    let nonce: [u8; 12] = data[16..28].try_into().expect("checked nonce length");
    let plaintext = key(primary, &data[..16])
        .open_in_place(Nonce::assume_unique_for_key(nonce), Aad::empty(), &mut body)
        .map_err(|_| GoText::from(CORRUPT))?;
    startup_rotation_json::parse(plaintext)
        .map_err(|error| error.with_prefix("unmarshal fallback entries: "))
}

fn persist(path: &Path, entries: &Records, primary: &[u8]) -> Result<(), GoText> {
    startup_fs::mkdir_private(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    let mut salt = [0; 16];
    getrandom::fill(&mut salt).map_err(|error| {
        GoText::from(format!("encrypt fallback secrets: generate salt: {error}"))
    })?;
    let mut nonce = [0; 12];
    getrandom::fill(&mut nonce).map_err(|error| {
        GoText::from(format!("encrypt fallback secrets: generate nonce: {error}"))
    })?;
    let mut body = startup_rotation_json::render(entries).into_bytes();
    key(primary, &salt)
        .seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::empty(), &mut body)
        .map_err(|_| GoText::from("encrypt fallback secrets: seal failed"))?;
    let mut payload = salt.to_vec();
    payload.extend(nonce);
    payload.extend(body);
    startup_fs::write_private(path, &payload)
}

fn warning(writer: &mut dyn Write, prefix: &[u8], text: &GoText) {
    let mut bytes = prefix.to_vec();
    bytes.extend(text.as_ref());
    bytes.push(b'\n');
    let _ = writer.write(&bytes);
}

pub(super) fn load(secret_path: &Path, primary: &[u8], warnings: &mut dyn Write) -> Vec<Entry> {
    let path = path(secret_path);
    let bytes = match startup_fs::read_file(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            warning(
                warnings,
                b"warning: failed to load fallback JWT secrets: ",
                &error.text,
            );
            return Vec::new();
        }
    };
    let entries = match decrypt(&bytes, primary) {
        Ok(entries) => entries,
        Err(original) => match startup_rotation_json::parse(&bytes) {
            Ok(entries) => {
                if let Err(error) = persist(&path, &entries, primary) {
                    warning(
                        warnings,
                        b"warning: failed to migrate plaintext fallback secrets to encrypted: ",
                        &error,
                    );
                }
                entries
            }
            Err(_) => {
                warning(
                    warnings,
                    b"warning: failed to load fallback JWT secrets: ",
                    &original,
                );
                return Vec::new();
            }
        },
    };
    let count = entries.entries.len();
    let now = Utc::now();
    let entries: Vec<_> = entries
        .entries
        .into_iter()
        .filter(|entry| entry.expires > now)
        .collect();
    let valid = Records {
        nil: entries.is_empty(),
        entries,
    };
    if valid.entries.len() != count
        && let Err(error) = persist(&path, &valid, primary)
    {
        warning(
            warnings,
            b"warning: failed to purge expired fallback secrets: ",
            &error,
        );
    }
    valid.entries
}

#[cfg(test)]
mod tests {
    #[test]
    fn derives_extension_from_the_final_component() {
        assert_eq!(
            super::path(std::path::Path::new("dir.dot/key")),
            std::path::Path::new("dir.dot/key.secrets")
        );
        assert_eq!(
            super::path(std::path::Path::new("dir/key.secret")),
            std::path::Path::new("dir/key.secrets")
        );
    }
}
