// Grok credential discovery is read-only: no refresh tokens, no writeback.
fn grok() -> Provider {
    let resolved = resolve_env("GROK_ACCESS_TOKEN");
    if let Some(raw) = env_raw("GROK_ACCESS_TOKEN") {
        return match resolved {
            Ok(Some((source, token))) => grok_from_token(&token, source, chrono::Utc::now()),
            _ => grok_unavailable(
                "missing",
                "Could not resolve GROK_ACCESS_TOKEN; check the credential reference",
                Some(secret_reference_source(&raw).into()),
            ),
        };
    }
    let path = env_path("GROK_HOME", home().join(".grok")).join("auth.json");
    grok_from_file(&path, chrono::Utc::now())
}

fn grok_unavailable(status: &str, detail: &str, source: Option<String>) -> Provider {
    Provider::new(
        "grok",
        "Grok",
        vec!["GROK_ACCESS_TOKEN", "auth.json"],
        Vec::new(),
        AuthStatus {
            status: status.into(),
            detail: detail.into(),
            source,
        },
    )
}

const GROK_LOGIN_HINT: &str =
    "Grok auth expired. Run `grok login` again; usage never refreshes or writes credentials.";

#[allow(clippy::cast_precision_loss)]
fn grok_token_expired(token: &str, now: chrono::DateTime<chrono::Utc>) -> bool {
    jwt_expiry(token).is_some_and(|expiry| expiry <= now.timestamp() as f64)
}

fn grok_from_token(token: &str, source: String, now: chrono::DateTime<chrono::Utc>) -> Provider {
    let token = token.trim();
    if token.is_empty() || token.chars().any(char::is_control) {
        return grok_unavailable(
            "missing",
            "Invalid GROK_ACCESS_TOKEN; supply an access token",
            Some(source),
        );
    }
    if grok_token_expired(token, now) {
        return grok_unavailable("expired", GROK_LOGIN_HINT, Some(source));
    }
    Provider::new(
        "grok",
        "Grok",
        vec!["GROK_ACCESS_TOKEN", "auth.json"],
        vec![(source.clone(), token.into())],
        available("Grok access token configured", source),
    )
}

#[derive(Deserialize)]
struct GrokAuthEntry {
    key: Option<String>,
    expires_at: Option<String>,
    expires: Option<String>,
}

fn grok_from_file(path: &Path, now: chrono::DateTime<chrono::Utc>) -> Provider {
    let missing = || {
        grok_unavailable(
            "missing",
            "Grok not logged in. Run `grok login`.",
            Some("file".into()),
        )
    };
    let Some(data) = read_limited(path) else {
        return missing();
    };
    let Ok(entries) =
        serde_json::from_slice::<std::collections::BTreeMap<String, GrokAuthEntry>>(&data)
    else {
        return grok_unavailable(
            "missing",
            "Grok auth invalid. Run `grok login` again.",
            Some("file".into()),
        );
    };
    let mut token = String::new();
    let mut expired = false;
    for entry in entries.values() {
        let candidate = entry.key.as_deref().unwrap_or_default().trim();
        if candidate.is_empty() || candidate.chars().any(char::is_control) {
            continue;
        }
        if !token.is_empty() && token != candidate {
            return grok_unavailable(
                "missing",
                "Multiple Grok accounts; select one explicitly with GROK_ACCESS_TOKEN",
                Some("file".into()),
            );
        }
        token = candidate.into();
        let expiry = chrono::DateTime::parse_from_rfc3339(
            entry.expires_at.as_deref().unwrap_or_default().trim(),
        )
        .or_else(|_| {
            chrono::DateTime::parse_from_rfc3339(
                entry.expires.as_deref().unwrap_or_default().trim(),
            )
        });
        expired |= expiry.is_ok_and(|expiry| expiry <= now) || grok_token_expired(&token, now);
    }
    if expired {
        return grok_unavailable("expired", GROK_LOGIN_HINT, Some("file".into()));
    }
    if token.is_empty() {
        return missing();
    }
    let mut provider = grok_from_token(&token, "file".into(), now);
    provider.auth_status.detail = "Signed in via Grok CLI auth store (read-only)".into();
    provider
}

#[cfg(test)]
#[path = "grok_tests.rs"]
mod grok_tests;
