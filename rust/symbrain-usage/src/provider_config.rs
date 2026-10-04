//! Provider credential state machine and per-source auth texts.
//!
//! Mirrors `internal/usage/{claude,codex,copilot,cursor,kimi,moonshot,nous,
//! opencode,openrouter,antigravity}.go`, including the exact missing/expired/
//! available texts and the source tag each state reports. Resolution order per
//! provider is: environment variable (symvault/keychain capable), then the
//! provider's own credential file, then — Claude on macOS only — the login
//! keychain. Claude and Codex files use Go-compatible native decoding; distinct
//! nondefault Claude tokens retain Go's unspecified map selection. Copilot,
//! Kimi CLI, and Hermes sources use their proven native contracts at default
//! or supported home paths. Supported providers may be combined; any unproven
//! source keeps the report on Go.

#[cfg(test)]
use super::Value;
use super::provider_requests::{trusted_https_url, validated_base};
use super::{AuthStatus, MAX_CREDENTIAL_FILE_BYTES, Provider};
use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::env;
use std::fmt;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// The service Claude Code stores its OAuth credentials under in the macOS
/// login keychain. Current versions append a per-installation hex suffix that
/// is not derivable from disk, so the bare name is tried first and every
/// suffixed variant found afterwards.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
/// Reading an item this binary is not on the ACL of raises an approval panel
/// that blocks the subprocess until answered; a usage report must not hang on
/// an unattended machine.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const CLAUDE_KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(20);
/// Bound for the prompt-free macOS Keychain listing.
#[cfg(target_os = "macos")]
const MAX_PROBE_OUTPUT_BYTES: u64 = 64 * 1024;

include!("provider_config/environment.rs");
include!("provider_config/commands.rs");
include!("provider_config/references.rs");
include!("provider_config/credential_json.rs");
include!("provider_config/credential_path.rs");
include!("provider_config/files.rs");
include!("provider_config/claude_file.rs");
include!("provider_config/codex_file.rs");
include!("provider_config/copilot_file.rs");
include!("provider_config/kimi_nous.rs");
include!("provider_config/hermes.rs");
include!("provider_config/jwt.rs");
include!("provider_config/keychain.rs");
include!("provider_config/registry.rs");
include!("provider_config/routing.rs");
include!("provider_config/accounts.rs");
include!("provider_config/other_accounts.rs");

#[cfg(test)]
#[path = "provider_config_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "provider_file_oracle_tests.rs"]
mod provider_file_oracle_tests;

#[cfg(test)]
#[path = "copilot_kimi_oracle_tests.rs"]
mod copilot_kimi_oracle_tests;
