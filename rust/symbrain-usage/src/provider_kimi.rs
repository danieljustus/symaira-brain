//! Kimi Code credential walk and CLI expiry handling (#931).
//!
//! Credential contract of `GET <KIMI_CODE_BASE_URL>/coding/v1/usages`, tried in
//! this order, each as `Authorization: Bearer`:
//!
//! 1. `api` — `KIMI_CODE_API_KEY` (a Kimi Code API key).
//! 2. `cli` — the access token the Kimi Code CLI stored. It lives 15 minutes
//!    and only the CLI may rotate it, so an idle CLI login is *expected* to be
//!    expired; the file is never written and the refresh token never read.
//! 3. `web` — `KIMI_AUTH_TOKEN` against the kimi.com billing service.
//!
//! `KIMI_CODE_API_KEY` / `KIMI_AUTH_TOKEN` accept `symvault://`, `vault://`,
//! `env://` and `keychain://` references (`resolve_env`). A reference that
//! fails to resolve is only reported when it leaves the provider unconfigured;
//! next to a present CLI token the sealed Go oracle
//! (`kimi-error-api-retains-cli`) pins the failure as dropped, so it cannot be
//! added to the chain without re-sealing. Expired CLI token: handled below
//! (#975), never written or refreshed.
//!
//! A Moonshot Open Platform key is a different product with its own provider
//! (`moonshot`); this endpoint is not documented to accept it. Which API-key
//! products the live endpoint accepts can only be confirmed against the real
//! service. The per-strategy 401 texts of the other two strategies are pinned
//! by the frozen Go oracle, so only the CLI expiry gets a specific message.
use super::{Provider, UsageError, request_for};
use crate::{UsageSnapshot, transport::Cancellation, transport::Transport};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const CLI_EXPIRED: &str = "Kimi Code CLI login expired — run the Kimi CLI once to refresh it";

impl Provider {
    pub(super) fn fetch_kimi(
        &self,
        transport: &Arc<dyn Transport>,
        cancel: &Cancellation,
    ) -> Result<UsageSnapshot, UsageError> {
        let mut errors = Vec::new();
        for (source, credential) in &self.credentials {
            let cli_expired = source == "cli" && self.kimi_cli_is_expired();
            if cli_expired {
                // The stored token declares itself expired: skip the request.
                errors.push(UsageError::exact(CLI_EXPIRED));
                continue;
            }
            let req = request_for(self, source, credential, None);
            match super::fetch_one(self, transport, cancel, source, req) {
                Ok(v) => return Ok(v),
                Err(e) => errors.push(e),
            }
        }
        Err(UsageError::chain(&self.id, errors))
    }

    fn kimi_cli_is_expired(&self) -> bool {
        self.kimi_cli_expires_at.is_some_and(|expiry| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .is_ok_and(|now| now.as_secs_f64() >= expiry)
        })
    }
}
