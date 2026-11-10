//! Account-safe attribution for local Claude and Codex logins (#1000).
//!
//! A usage row always belongs to exactly one credential. When more than one
//! local login exists, or the credential in use cannot be tied to one, the
//! report names each discovered account by a stable, non-secret label and says
//! which one the row reads. Usage of different accounts is never merged: only
//! the account in state `used` contributes to the row's snapshot.
//!
//! Labels are `<provider>-<8 hex of SHA-256(account id)>`, or
//! `claude-keychain-<suffix>` for a Claude Code keychain item whose owner id is
//! not on disk. They never contain a token, an e-mail address or a raw id.
//! The field is emitted only when there is something to disambiguate, so
//! single-login reports keep their frozen shape.
use ring::digest::{SHA256, digest};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

/// Keychain service Claude Code stores its default login under.
pub(crate) const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

/// One discovered local account, without credential material.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRef {
    /// Stable non-secret label, or `unattributed`.
    pub label: String,
    /// Where it was found: `claude_cli`, `claude_cli_keychain`,
    /// `claude_desktop`, `codex_auth_file` or the credential source in use.
    pub source: String,
    /// `used` (this row reads it), `not_read`, `candidate` (may be the owner of
    /// the unattributed credential) or `unattributed`.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

pub(crate) fn label(prefix: &str, id: &str) -> String {
    let hash = digest(&SHA256, id.trim().to_lowercase().as_bytes());
    let hex = hash.as_ref()[..4].iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    });
    format!("{prefix}-{hex}")
}

/// Everything read-only discovery learned about Claude; no credential values.
#[derive(Debug, Default)]
pub(crate) struct ClaudeScan {
    pub oauth_source: Option<String>,
    pub admin_key: bool,
    /// Keychain service the OAuth token was read from.
    pub keychain_service: Option<String>,
    /// Claude Code keychain service names (attributes only).
    pub keychain_services: Vec<String>,
    /// `CLAUDE_CONFIG_DIR` points the CLI at a login this reader does not read.
    pub custom_config_dir: bool,
    pub cli_account_id: Option<String>,
    pub desktop_account_id: Option<String>,
}

/// Everything read-only discovery learned about Codex; no credential values.
/// Single Codex home only: the active `CODEX_HOME` or the default `~/.codex`;
/// further Codex logins are deliberately not discovered (#1000).
#[derive(Debug, Default)]
pub(crate) struct CodexScan {
    pub token_source: Option<String>,
    pub file_account_id: Option<String>,
}

fn entry(label: &str, source: &str, state: &str) -> AccountRef {
    AccountRef {
        label: label.into(),
        source: source.into(),
        state: state.into(),
        detail: None,
    }
}

/// Marks `used` among `found`, the rest `not_read`; an unattributed credential
/// is listed first and every discovered account becomes a `candidate`.
fn finish(found: Vec<AccountRef>, used: Result<String, (String, &str)>) -> Vec<AccountRef> {
    match used {
        Ok(used) => found
            .into_iter()
            .map(|mut a| {
                a.state = if a.label == used { "used" } else { "not_read" }.into();
                a
            })
            .collect(),
        Err((source, why)) => {
            let mut head = entry("unattributed", &source, "unattributed");
            head.detail = Some(why.into());
            let mut out = vec![head];
            out.extend(found.into_iter().map(|mut a| {
                a.state = "candidate".into();
                a
            }));
            out
        }
    }
}

fn push(found: &mut Vec<AccountRef>, label: &str, source: &str) {
    if !found.iter().any(|a| a.label == label) {
        found.push(entry(label, source, "not_read"));
    }
}

pub(crate) fn claude_accounts(scan: &ClaudeScan) -> Vec<AccountRef> {
    let source = scan.oauth_source.as_deref();
    if source.is_none() && !scan.admin_key {
        return Vec::new();
    }
    let default = scan.cli_account_id.as_deref().map(|id| label("claude", id));
    let mut found = Vec::new();
    if let Some(default) = &default {
        push(&mut found, default, "claude_cli");
    }
    let suffix_label = |service: &str| {
        service
            .strip_prefix(&format!("{CLAUDE_KEYCHAIN_SERVICE}-"))
            .map(|suffix| format!("claude-keychain-{suffix}"))
    };
    for service in &scan.keychain_services {
        if let Some(label) = suffix_label(service) {
            push(&mut found, &label, "claude_cli_keychain");
        }
    }
    if let Some(id) = &scan.desktop_account_id {
        push(&mut found, &label("claude", id), "claude_desktop");
    }
    let used = if scan.admin_key {
        Err("an Admin API key is tried before the OAuth login")
    } else if scan.custom_config_dir {
        Err("CLAUDE_CONFIG_DIR selects a login that is not read here")
    } else {
        match (source, scan.keychain_service.as_deref()) {
            (Some("file"), _) | (Some("keychain"), Some(CLAUDE_KEYCHAIN_SERVICE)) => {
                default.ok_or("~/.claude.json carries no account id")
            }
            (Some("keychain"), Some(service)) => {
                suffix_label(service).ok_or("unknown keychain service")
            }
            (Some("keychain"), None) => Err("keychain item could not be identified"),
            _ => Err("token comes from the environment or a vault reference"),
        }
    };
    let used = match used {
        Ok(label) => {
            push(&mut found, &label, "claude_cli_keychain");
            Ok(label)
        }
        Err(why) => Err((source.unwrap_or("api_key").to_owned(), why)),
    };
    // One account and a clear owner: nothing to disambiguate.
    if (found.len() < 2 && used.is_ok()) || found.is_empty() {
        return Vec::new();
    }
    finish(found, used)
}

pub(crate) fn codex_accounts(scan: &CodexScan) -> Vec<AccountRef> {
    let Some(source) = scan.token_source.as_deref() else {
        return Vec::new();
    };
    let file = scan.file_account_id.as_deref().map(|id| label("codex", id));
    // With a single file login and a token read from it there is no ambiguity.
    let (Some(file), true) = (file, source != "file") else {
        return Vec::new();
    };
    let found = vec![entry(&file, "codex_auth_file", "not_read")];
    finish(
        found,
        Err((
            source.to_owned(),
            "token comes from the environment or a vault reference",
        )),
    )
}

#[cfg(test)]
#[path = "accounts_tests.rs"]
mod tests;
