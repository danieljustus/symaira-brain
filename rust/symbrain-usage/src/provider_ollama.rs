use super::{Provider, UsageError};
use crate::transport::{Cancellation, Request, Transport};
use crate::{AuthStatus, UsageSnapshot};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

fn key_error() -> UsageError {
    UsageError::payload(
        "ollama",
        "unusable local Ollama signing key (requires an owned, private, unencrypted Ed25519 key)",
    )
}

impl Provider {
    /// Explicit opt-in only. Does not inspect credentials or run Ollama.
    #[must_use]
    pub fn ollama() -> Self {
        let mut provider = Self::new(
            "ollama",
            "Ollama",
            vec!["file"],
            Vec::new(),
            AuthStatus {
                status: "partial".into(),
                detail: "Ollama Cloud explicitly enabled; signing key checked on refresh".into(),
                source: Some("file".into()),
            },
        );
        provider.configured = true;
        provider
    }

    pub(super) fn fetch_ollama(
        &self,
        transport: &Arc<dyn Transport>,
        cancel: &Cancellation,
    ) -> Result<UsageSnapshot, UsageError> {
        let home = std::env::var_os("HOME").ok_or_else(key_error)?;
        self.fetch_ollama_at(
            &std::path::PathBuf::from(home).join(".ollama/id_ed25519"),
            transport,
            cancel,
        )
    }

    fn fetch_ollama_at(
        &self,
        path: &Path,
        transport: &Arc<dyn Transport>,
        cancel: &Cancellation,
    ) -> Result<UsageSnapshot, UsageError> {
        let data = read_key(path)?;
        let (key, public) = parse_key(&data).ok_or_else(key_error)?;
        let request = signed_request(&key, &public, chrono::Utc::now().timestamp());
        let response = transport
            .request_with_cancel(request, cancel)
            .map_err(|_| UsageError::transport(&self.id, "signed request failed"))?;
        if !(200..300).contains(&response.status) {
            return Err(super::status_error(
                "ollama",
                response.status,
                &response.headers,
            ));
        }
        parse_balance(&response.body, chrono::Utc::now())
    }
}

fn signed_request(key: &Ed25519KeyPair, public: &[u8], timestamp: i64) -> Request {
    let uri = format!("/api/balance?ts={timestamp}");
    let signature = key.sign(format!("GET,{uri}").as_bytes());
    Request {
        provider_id: "ollama".into(),
        method: "GET".into(),
        url: format!("https://ollama.com{uri}"),
        headers: BTreeMap::from([
            (
                "Authorization".into(),
                format!(
                    "{}:{}",
                    STANDARD.encode(public),
                    STANDARD.encode(signature.as_ref())
                ),
            ),
            ("Accept".into(), "application/json".into()),
        ]),
        body: None,
    }
}

#[cfg(unix)]
fn read_key(path: &Path) -> Result<Vec<u8>, UsageError> {
    use rustix::fs::{Mode, OFlags, open};
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    let fd = open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| key_error())?;
    let file = std::fs::File::from(fd);
    let meta = file.metadata().map_err(|_| key_error())?;
    if !meta.is_file()
        || meta.uid() != rustix::process::geteuid().as_raw()
        || meta.nlink() != 1
        || meta.mode() & 0o7077 != 0
        || meta.len() == 0
        || meta.len() > 65536
    {
        return Err(key_error());
    }
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| key_error())?;
    if bytes.len() > 65536 {
        return Err(key_error());
    }
    Ok(bytes)
}
#[cfg(not(unix))]
fn read_key(_: &Path) -> Result<Vec<u8>, UsageError> {
    Err(key_error())
}

// Only the bounded, unencrypted single-key OpenSSH container is accepted.
fn parse_key(pem: &[u8]) -> Option<(Ed25519KeyPair, Vec<u8>)> {
    if pem.len() > 65536 {
        return None;
    }
    let text = std::str::from_utf8(pem).ok()?;
    let body = text
        .strip_prefix(concat!("-----BEGIN OPENSSH ", "PRIVATE KEY-----\n"))?
        .trim_end()
        .strip_suffix("-----END OPENSSH PRIVATE KEY-----")?;
    let blob = STANDARD
        .decode(body.split_whitespace().collect::<String>())
        .ok()?;
    let mut reader = Wire(blob.strip_prefix(b"openssh-key-v1\0")?);
    if reader.string()? != b"none"
        || reader.string()? != b"none"
        || !reader.string()?.is_empty()
        || reader.u32()? != 1
    {
        return None;
    }
    let public = reader.string()?;
    let private = reader.string()?;
    if !reader.0.is_empty() || private.len() % 8 != 0 {
        return None;
    }
    let mut secret = Wire(private);
    if secret.u32()? != secret.u32()? || secret.string()? != b"ssh-ed25519" {
        return None;
    }
    let repeated = secret.string()?;
    let seed_public = secret.string()?;
    secret.string()?; // comment
    if seed_public.len() != 64
        || repeated.len() != 32
        || secret.0.is_empty()
        || secret.0.len() > 8
        || !secret
            .0
            .iter()
            .enumerate()
            .all(|(i, b)| usize::from(*b) == i + 1)
    {
        return None;
    }
    let key =
        Ed25519KeyPair::from_seed_and_public_key(&seed_public[..32], &seed_public[32..]).ok()?;
    let mut outer = Wire(public);
    if outer.string()? != b"ssh-ed25519"
        || outer.string()? != key.public_key().as_ref()
        || !outer.0.is_empty()
        || repeated != key.public_key().as_ref()
    {
        return None;
    }
    Some((key, public.to_vec()))
}
struct Wire<'a>(&'a [u8]);
impl<'a> Wire<'a> {
    fn u32(&mut self) -> Option<u32> {
        let (value, rest) = self.0.split_at_checked(4)?;
        self.0 = rest;
        Some(u32::from_be_bytes(value.try_into().ok()?))
    }
    fn string(&mut self) -> Option<&'a [u8]> {
        let size = usize::try_from(self.u32()?).ok()?;
        let (value, rest) = self.0.split_at_checked(size)?;
        self.0 = rest;
        Some(value)
    }
}

pub(crate) fn parse_balance(
    body: &[u8],
    now: chrono::DateTime<chrono::Utc>,
) -> Result<UsageSnapshot, UsageError> {
    use crate::UsageMeter;
    use serde_json::Value;
    fn number(value: &Value) -> Option<f64> {
        value
            .as_f64()
            .or_else(|| value.as_str()?.parse().ok())
            .filter(|v| v.is_finite())
    }
    fn amount(value: f64) -> String {
        if value == 0.0 {
            "0".into()
        } else {
            value.to_string()
        }
    }
    fn date(value: &Value) -> Option<chrono::DateTime<chrono::FixedOffset>> {
        chrono::DateTime::parse_from_rfc3339(value.as_str()?).ok()
    }
    let root: Value = serde_json::from_slice(body)
        .map_err(|_| UsageError::payload("ollama", "invalid balance response"))?;
    let included = &root["included"];
    let mut snapshot = UsageSnapshot {
        provider_id: "ollama".into(),
        fetched_at: now,
        source: "api".into(),
        ..UsageSnapshot::default()
    };
    for (field, label) in [("session", "Session"), ("weekly", "Weekly")] {
        if let Some(remaining) = number(&included[field]["remaining_percent"]) {
            snapshot.meters.push(UsageMeter {
                label: label.into(),
                used: Some(amount((100.0 - remaining).clamp(0.0, 100.0))),
                limit: Some("100".into()),
                unit: "%".into(),
                resets_at: date(&included[field]["resets_at"]),
            });
        }
    }
    if let (Some(allowance), Some(balance)) = (
        number(&included["allowance_usd"]).filter(|v| *v > 0.0),
        number(&included["balance_usd"]),
    ) {
        snapshot.meters.push(UsageMeter {
            label: "Monthly".into(),
            used: Some(amount((allowance - balance).clamp(0.0, allowance))),
            limit: Some(amount(allowance)),
            unit: "USD".into(),
            resets_at: date(&included["period"]["until"]),
        });
    }
    if snapshot.meters.is_empty() {
        return Err(UsageError::payload(
            "ollama",
            "response contained no usable included quota",
        ));
    }
    if let Some(balance) = number(&root["purchased"]["balance_usd"]) {
        snapshot.balance = Some(amount(balance.max(0.0)));
        snapshot.currency = Some("USD".into());
    }
    Ok(snapshot)
}

#[cfg(test)]
#[path = "ollama_tests.rs"]
mod tests;
