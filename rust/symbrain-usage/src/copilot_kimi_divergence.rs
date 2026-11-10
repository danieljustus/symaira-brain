// Accepted divergences of the Copilot/Kimi replay from the sealed Go oracle.
//
// Registry of exactly one case (maintainer decision 2026-11-10, issue #931):
// the frozen `kimi-error-api-retains-cli` record drops an unresolved
// `KIMI_CODE_API_KEY` reference when a Kimi CLI token keeps the provider
// configured. Rust reports the reference next to the CLI failure instead. The
// sealed fixture stays byte-identical; this table projects the one changed
// field (`providers[0].error`) of that one case and asserts both sides, so
// any other drift - a different case, a different field, a changed frozen
// record, or a different new text - still fails the replay.

/// The frozen Go bytes of `providers[0].error` for the diverging case.
const KIMI_GO_ERROR: &str = "all AI usage fallbacks failed: Kimi rejected the login (HTTP 401). Check the API access or sign in with the Kimi Code CLI again.";
/// What Rust reports: the unresolved key reference leads the chain. Only the
/// reference and the resolver's message appear, never a secret value.
const KIMI_RUST_ERROR: &str = "all AI usage fallbacks failed: resolve KIMI_CODE_API_KEY: secret resolution failed for env://USAGE_LOCAL_FILES_ABSENT: environment variable USAGE_LOCAL_FILES_ABSENT is not set (reference env://USAGE_LOCAL_FILES_ABSENT); set env var  as fallback or install symvault; Kimi rejected the login (HTTP 401). Check the API access or sign in with the Kimi Code CLI again.";

/// Every accepted divergence: `(case id, frozen Go error, Rust error)`.
const ACCEPTED_DIVERGENCES: [(&str, &str, &str); 1] = [(
    "kimi-error-api-retains-cli",
    KIMI_GO_ERROR,
    KIMI_RUST_ERROR,
)];

/// Rewrites `expected` (a frozen record's report) to the accepted Rust value
/// when `id` is registered; returns whether a divergence was applied.
fn apply_accepted_divergence(id: &str, expected: &mut serde_json::Value) -> bool {
    let Some((_, go, rust)) = ACCEPTED_DIVERGENCES.iter().find(|(case, ..)| *case == id) else {
        return false;
    };
    let provider = &mut expected["providers"][0];
    assert_eq!(
        provider["error"].as_str(),
        Some(*go),
        "{id}: the sealed record no longer carries the Go bytes this divergence was accepted against"
    );
    provider["error"] = (*rust).into();
    true
}
