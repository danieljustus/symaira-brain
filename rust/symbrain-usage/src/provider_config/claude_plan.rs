// Claude plan label from the login that was already resolved (#999). The plan
// rides along with the credential the provider reads anyway: no extra request
// and no further credential source.

/// Plan of the keychain entry the real reader parsed last; handed over once by
/// `take_claude_keychain_plan` so the blob is never fetched a second time.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
static CLAUDE_KEYCHAIN_PLAN: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn remember_claude_keychain_plan(blob: &[u8]) {
    if let Ok(mut slot) = CLAUDE_KEYCHAIN_PLAN.lock() {
        *slot = claude_plan_from_credentials(blob);
    }
}

fn take_claude_keychain_plan() -> Option<String> {
    CLAUDE_KEYCHAIN_PLAN.lock().ok().and_then(|mut slot| slot.take())
}

fn claude_file_plan(path: &Path) -> Option<String> {
    claude_plan_from_credentials(&read_provider_credentials(path)?)
}

/// `claudeAiOauth` (Claude Code), else the `oauthAccount` entry the token
/// selection prefers (`default`, then the first account in sorted order).
pub(crate) fn claude_plan_from_credentials(contents: &[u8]) -> Option<String> {
    let root: serde_json::Value = serde_json::from_slice(contents).ok()?;
    if let Some(plan) = root.get("claudeAiOauth").and_then(claude_plan_from_oauth) {
        return Some(plan);
    }
    let accounts = root.get("oauthAccount")?.as_object()?;
    accounts
        .get("default")
        .and_then(claude_plan_from_oauth)
        .or_else(|| accounts.values().find_map(claude_plan_from_oauth))
}

fn claude_plan_from_oauth(oauth: &serde_json::Value) -> Option<String> {
    format_claude_plan(
        oauth.get("subscriptionType")?.as_str()?,
        oauth.get("rateLimitTier").and_then(serde_json::Value::as_str),
    )
}

/// `max` + `default_claude_max_5x` -> `Max 5x`; words are title-cased.
pub(crate) fn format_claude_plan(subscription: &str, tier: Option<&str>) -> Option<String> {
    let subscription = subscription.trim();
    if subscription.is_empty() {
        return None;
    }
    let base = subscription
        .split(' ')
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ");
    Some(match tier.and_then(tier_multiplier) {
        Some(multiplier) => format!("{base} {multiplier}"),
        None => base,
    })
}

// The first `\d+x` run of the rate-limit tier.
fn tier_multiplier(tier: &str) -> Option<&str> {
    let bytes = tier.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if bytes.get(index) == Some(&b'x') {
            return Some(&tier[start..=index]);
        }
    }
    None
}
