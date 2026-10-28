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
    needs_go_fallback_for_selection(&ProviderSelection::default())
}

/// Classifies only selected providers; excluded providers perform no discovery.
#[must_use]
pub fn needs_go_fallback_for_selection(selection: &ProviderSelection) -> bool {
    selected_fallback_with_probes(selection, claude_keychain_present, kimi_cli_home)
}

fn selected_fallback_with_probes(
    selection: &ProviderSelection,
    keychain_present: impl FnOnce() -> bool,
    kimi_home: impl FnOnce() -> PathBuf,
) -> bool {
    let claude_oauth_env = selection
        .contains("claude")
        .then(|| env_raw("ANTHROPIC_OAUTH_TOKEN"))
        .flatten();
    let opencode_env = selection
        .contains("opencode")
        .then(|| env_raw("OPENCODE_COOKIE"))
        .flatten();
    let opencode_workspace_override = env_raw("OPENCODE_WORKSPACE_ID");
    let unsupported_base = [
        ("kimi", "KIMI_CODE_BASE_URL"),
        ("nous", "HERMES_PORTAL_BASE_URL"),
        ("openrouter", "OPENROUTER_API_URL"),
    ]
    .into_iter()
    .filter(|(id, _)| selection.contains(id))
    .filter_map(|(_, name)| env_raw(name))
    .any(|value| !supported_custom_base(&value));
    // Go resolves its home from USERPROFILE on Windows; Rust currently uses
    // HOME. Keep the entire usage report on Go if those roots differ so no
    // provider can silently miss a file-backed credential.
    if ["claude", "codex", "copilot", "kimi", "nous"]
        .iter()
        .any(|id| selection.contains(id))
        && usage_home_mismatch_requires_go()
    {
        return true;
    }
    if unsupported_base {
        return true;
    }
    // Both successful and failed environment resolution preempt file selection.
    if selection.contains("copilot")
        && env_raw("COPILOT_ACCESS_TOKEN").is_none()
        && copilot_file_token_candidate_in(&copilot_config_dir()).map_or(true, |token| {
            token.is_some_and(|value| value.bytes().any(|byte| byte.is_ascii_control()))
        })
    {
        return true;
    }
    let claude_file_path = home().join(".claude/.credentials.json");
    let claude_file_token = selection
        .contains("claude")
        .then(|| claude_file_token_in(&claude_file_path))
        .flatten();
    if selection.contains("claude") && claude_file_requires_go(&claude_file_path) {
        // Go iterates the account map in unspecified order. Retain its route
        // only when multiple distinct nondefault tokens can be selected.
        return true;
    }
    let nous_path = nous_auth_path();
    if selection.contains("nous") && nous_file_requires_go(&nous_path) {
        // Go float-to-int overflow is architecture-dependent. Preserve that
        // remaining gate until native target receipts define the contract.
        return true;
    }
    // On macOS Go's Kimi reader and the native reader share one
    // capability-rooted contract (symlinks, non-regular files, oversize and
    // permission failures all yield no CLI token; a FIFO is a deliberate
    // native non-blocking divergence because Go's open blocks), so unsafe file
    // shapes need no Go route there. See kimi_file_requires_go for the other
    // targets. A token with an internal control byte always routes to Go.
    let kimi_cli = if selection.contains("kimi") {
        // Home selection itself probes credential-file existence.
        let kimi_home = kimi_home();
        let kimi_path = credential_join(&kimi_home, "credentials/kimi-code.json");
        let token = kimi_file_token_candidate(&kimi_path);
        if kimi_file_requires_go(&kimi_path)
            || token
                .as_ref()
                .is_some_and(|token| token.bytes().any(|byte| byte.is_ascii_control()))
            || !kimi_device_id_is_native(&credential_join(&kimi_home, "device_id"))
        {
            return true;
        }
        token
    } else {
        None
    };
    // Without a cookie Go has no request strategy, so the workspace is only
    // reported as configured and its spelling cannot affect an HTTP request.
    if opencode_env.is_some()
        && env_raw("OPENCODE_GO_API_KEY").is_none()
        && opencode_workspace_override
            .as_deref()
            .is_some_and(|workspace| !supported_opencode_workspace(workspace))
    {
        return true;
    }
    needs_go_fallback_for(&UsageFallbackSignals {
        claude_oauth_env: claude_oauth_env.as_deref(),
        copilot_file: None,
        kimi_cli: kimi_cli.as_deref(),
        claude_file: claude_file_token.as_deref(),
        other_provider_env: false,
        other_credential_source: false,
        // Go skips its Keychain read when either the OAuth environment source
        // is present (even if resolving it fails) or the file supplied a
        // token. Avoid listing Keychain attributes in either case.
        local_provider_present: selection.contains("claude")
            && claude_oauth_env.is_none()
            && claude_file_token.is_none()
            && keychain_present(),
    })
}

/// Keeps reports native for the proven portable sources once every configured
/// source is handled by the same provider constructors. Distinct eligible Copilot tokens, unsupported file/device access,
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
    // Environment references use the shared resolver; provider-file tokens
    // remain literal. Ambiguous Copilot entries are gated before this helper.
    // Copilot and Kimi files are literal token sources, like Go's readers.
    let _ = (signals.copilot_file, signals.kimi_cli);
    if signals.other_provider_env
        || signals.other_credential_source
        || (signals.local_provider_present
            && signals.claude_oauth_env.is_none()
            && signals.claude_file.is_none())
    {
        return true;
    }
    false
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

#[cfg(test)]
mod selection_tests {
    use super::*;

    #[test]
    fn excluded_provider_probes_are_never_called() {
        let selection = ProviderSelection::parse("antigravity").unwrap();
        assert!(!selected_fallback_with_probes(
            &selection,
            || panic!("excluded Claude Keychain probe"),
            || panic!("excluded Kimi credential-directory probe"),
        ));
    }
}
