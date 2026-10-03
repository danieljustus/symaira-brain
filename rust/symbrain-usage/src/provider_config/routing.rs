// Native provider routing compatibility implementation.

/// Whether the report would read a stored credential outside the native
/// provider subset, i.e. whether the Go fallback is required.
///
/// The checks here are prompt-free: environment values, credential files, and
/// the keychain *listing* (attributes only, never a secret value). The local
/// Antigravity probe is performed by its native provider. File-backed routes
/// and home overrides require a deterministic
/// Go-equivalent subset; unsupported file sources and base/workspace
/// overrides stay on Go.
/// Multiple configured providers can be native together when each source is
/// proven. A workspace override alone supplies no `OpenCode` cookie or
/// strategy, so it cannot start a fetch.
#[must_use]
pub fn needs_go_fallback() -> bool {
    let claude_oauth_env = env_raw("ANTHROPIC_OAUTH_TOKEN");
    let opencode_env = env_raw("OPENCODE_COOKIE");
    let opencode_workspace_override = env_raw("OPENCODE_WORKSPACE_ID");
    let unsupported_base = [
        env_raw("KIMI_CODE_BASE_URL"),
        env_raw("HERMES_PORTAL_BASE_URL"),
        env_raw("OPENROUTER_API_URL"),
    ]
    .into_iter()
    .flatten()
    .any(|value| !supported_custom_base(&value));
    // Go resolves its home from USERPROFILE on Windows; Rust currently uses
    // HOME. Keep the entire usage report on Go if those roots differ so no
    // provider can silently miss a file-backed credential.
    if usage_home_mismatch_requires_go() {
        return true;
    }
    if unsupported_base {
        return true;
    }
    let Ok(copilot_file) = copilot_file_token_candidate_in(&copilot_config_dir()) else {
        return true;
    };
    let claude_file_path = home().join(".claude/.credentials.json");
    let claude_file_token = claude_file_token_in(&claude_file_path);
    if claude_file_requires_go(&claude_file_path) {
        // Go iterates the account map in unspecified order. Retain its route
        // only when multiple distinct nondefault tokens can be selected.
        return true;
    }
    let nous_path = nous_auth_path();
    if nous_file_requires_go(&nous_path) {
        // Go float-to-int overflow is architecture-dependent. Preserve that
        // remaining gate until native target receipts define the contract.
        return true;
    }
    let kimi_path = kimi_cli_home().join("credentials/kimi-code.json");
    let kimi_cli = kimi_file_token_candidate(&kimi_path);
    if path_may_exist(&kimi_path) && kimi_cli.is_none() {
        return true;
    }
    if !kimi_device_id_is_native(&kimi_cli_home().join("device_id")) {
        return true;
    }
    // Without a cookie Go has no request strategy, so the workspace is only
    // reported as configured and its spelling cannot affect an HTTP request.
    if opencode_env.is_some()
        && opencode_workspace_override
            .as_deref()
            .is_some_and(|workspace| !supported_opencode_workspace(workspace))
    {
        return true;
    }
    needs_go_fallback_for(&UsageFallbackSignals {
        claude_oauth_env: claude_oauth_env.as_deref(),
        copilot_file: copilot_file.as_deref(),
        kimi_cli: kimi_cli.as_deref(),
        claude_file: claude_file_token.as_deref(),
        other_provider_env: false,
        other_credential_source: false,
        // Go skips its Keychain read when either the OAuth environment source
        // is present (even if resolving it fails) or the file supplied a
        // token. Avoid listing Keychain attributes in either case.
        local_provider_present: claude_oauth_env.is_none()
            && claude_file_token.is_none()
            && claude_keychain_present(),
    })
}

/// Keeps reports native for the proven portable sources once every configured
/// source is handled by the same provider constructors. Unproven Copilot/Kimi files
/// and the automatic Claude Keychain source still keep Go in charge.
#[derive(Clone, Copy, Default)]
struct UsageFallbackSignals<'a> {
    claude_oauth_env: Option<&'a str>,
    copilot_file: Option<&'a str>,
    kimi_cli: Option<&'a str>,
    claude_file: Option<&'a str>,
    other_provider_env: bool,
    other_credential_source: bool,
    local_provider_present: bool,
}

fn needs_go_fallback_for(signals: &UsageFallbackSignals<'_>) -> bool {
    // Environment references share the secure source-bound resolver. Claude
    // and Codex file tokens are literal native inputs; unproven Copilot/Kimi
    // file references retain their eligibility gate.
    let credentials = [signals.copilot_file, signals.kimi_cli];
    if signals.other_provider_env
        || signals.other_credential_source
        || (signals.local_provider_present
            && signals.claude_oauth_env.is_none()
            && signals.claude_file.is_none())
    {
        return true;
    }
    credentials.into_iter().flatten().any(is_secret_reference)
}

/// Whether any Claude Code keychain service name exists, bare or suffixed.
///
/// Only attributes are listed — no secret is read, so deciding whether the
/// report stays on the shipped implementation cannot raise an approval panel.
#[cfg(target_os = "macos")]
fn claude_keychain_present() -> bool {
    claude_keychain_dump_names()
        .iter()
        .any(|name| valid_claude_service_name(name))
}

#[cfg(not(target_os = "macos"))]
fn claude_keychain_present() -> bool {
    false
}
