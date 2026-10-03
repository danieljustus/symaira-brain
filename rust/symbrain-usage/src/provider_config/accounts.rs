// Native provider accounts compatibility implementation.
fn claude() -> Provider {
    claude_with_keychain(claude_keychain_credential)
}

// Keep system Keychain access injectable privately, as in the Go constructor.
fn claude_with_keychain(read_keychain: impl FnOnce() -> Option<(String, SystemTime)>) -> Provider {
    let admin = resolve_env("ANTHROPIC_ADMIN_KEY");
    let oauth = resolve_env_or_file("ANTHROPIC_OAUTH_TOKEN", claude_file_token);
    let (admin_value, admin_error) = match admin {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    let (mut oauth_value, oauth_error) = match oauth {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    let mut oauth_expires_at = None;
    // Only tried when neither the env var nor the file produced a token: the
    // keychain read can raise an approval panel.
    if oauth_value.is_none()
        && oauth_error.is_none()
        && let Some((token, expires_at)) = read_keychain()
    {
        oauth_value = Some(("keychain".into(), token));
        oauth_expires_at = expires_at;
    }
    claude_from_resolved(
        admin_value,
        admin_error,
        oauth_value,
        oauth_error,
        oauth_expires_at,
    )
}

pub(crate) fn claude_from_resolved(
    admin_value: Option<(String, String)>,
    admin_error: Option<String>,
    oauth_value: Option<(String, String)>,
    oauth_error: Option<String>,
    oauth_expires_at: Option<SystemTime>,
) -> Provider {
    let mut credentials = Vec::new();
    if let Some((_, value)) = admin_value.clone() {
        // The Go strategy's request/report source is always "api"; the
        // resolved source (env, vault, and so on) belongs in AuthStatus only.
        credentials.push(("api".into(), value));
    }
    if let Some((_credential_source, value)) = oauth_value.clone() {
        // Go's Claude OAuth strategy always reports `oauth` for the fetched
        // snapshot, while AuthStatus separately retains env/file/keychain
        // provenance. Keep those two source labels distinct.
        credentials.push(("oauth".into(), value));
    }
    let mut provider = Provider::new(
        "claude",
        "Claude",
        vec![
            "ANTHROPIC_ADMIN_KEY",
            "ANTHROPIC_OAUTH_TOKEN",
            "file",
            "keychain",
        ],
        credentials,
        AuthStatus::default(),
    );
    provider.configured = admin_value.is_some() || oauth_value.is_some();
    provider.auth_status = if !provider.configured {
        match (admin_error, oauth_error) {
            (Some(error), _) | (None, Some(error)) => auth_error(&error),
            (None, None) => missing(
                "No Claude credentials found — add an admin key or sign in with the Claude CLI",
            ),
        }
    } else if let Some((source, _)) = oauth_value {
        if oauth_expires_at.is_some_and(|expiry| expiry < SystemTime::now()) {
            AuthStatus {
                status: "expired".into(),
                detail: "Claude Code OAuth token expired — sign in again with the Claude CLI"
                    .into(),
                source: Some(source),
            }
        } else {
            available("Signed in via Claude Code OAuth token", source)
        }
    } else {
        let source = admin_value.map_or_else(|| "env".to_owned(), |(source, _)| source);
        available("Admin API key from ANTHROPIC_ADMIN_KEY", source)
    };
    provider
}

fn codex() -> Provider {
    let home_dir = codex_home();
    let resolved = resolve_env_or_file("CODEX_ACCESS_TOKEN", || codex_file_token(&home_dir));
    let (value, error) = match resolved {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    codex_from_resolved(value, error, home_dir.join("auth.json").exists())
}

pub(crate) fn codex_from_resolved(
    value: Option<(String, String)>,
    error: Option<String>,
    auth_file_exists: bool,
) -> Provider {
    let mut provider = Provider::new(
        "codex",
        "Codex",
        vec!["CODEX_ACCESS_TOKEN", "auth.json"],
        value.clone().into_iter().collect(),
        AuthStatus::default(),
    );
    provider.configured = value.is_some();
    provider.auth_status = if let Some((source, _)) = value {
        available(
            "Signed in via Codex CLI OAuth (CODEX_ACCESS_TOKEN or auth.json)",
            source,
        )
    } else if let Some(error) = error {
        auth_error(&error)
    } else if auth_file_exists {
        AuthStatus {
            status: "expired".into(),
            detail: "Codex auth file found but no valid token — re-auth with the Codex CLI".into(),
            source: Some("file".into()),
        }
    } else {
        missing("No Codex credentials found — sign in with the Codex CLI")
    };
    provider
}

fn copilot() -> Provider {
    let resolved = resolve_env_or_file("COPILOT_ACCESS_TOKEN", copilot_file_token);
    let (value, error) = match resolved {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    copilot_from_resolved(value, error)
}

pub(crate) fn copilot_from_resolved(
    value: Option<(String, String)>,
    error: Option<String>,
) -> Provider {
    let mut provider = Provider::new(
        "copilot",
        "GitHub Copilot",
        vec!["COPILOT_ACCESS_TOKEN", "apps.json", "hosts.json"],
        value.clone().into_iter().collect(),
        AuthStatus::default(),
    );
    provider.configured = value.is_some();
    provider.auth_status = if let Some((source, _)) = value {
        available(
            "Signed in via GitHub Copilot (COPILOT_ACCESS_TOKEN or Copilot CLI)",
            source,
        )
    } else if let Some(error) = error {
        auth_error(&error)
    } else {
        missing("No Copilot token found — sign in with the Copilot CLI")
    };
    provider
}

fn cursor() -> Provider {
    let resolved = resolve_env("CURSOR_COOKIE");
    let (value, error) = match resolved {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    cursor_from_resolved(value, error)
}

pub(crate) fn cursor_from_resolved(
    value: Option<(String, String)>,
    error: Option<String>,
) -> Provider {
    let mut provider = Provider::new(
        "cursor",
        "Cursor",
        vec!["CURSOR_COOKIE"],
        value.clone().into_iter().collect(),
        AuthStatus::default(),
    );
    provider.configured = value.is_some();
    provider.auth_status = if let Some(error) = error {
        auth_error(&error)
    } else if let Some((source, _)) = value {
        available("Cookie configured (CURSOR_COOKIE)", source)
    } else {
        missing("No Cursor credentials found — set CURSOR_COOKIE or sign in to Cursor.app")
    };
    provider
}

fn kimi() -> Provider {
    let cli_home = kimi_cli_home();
    let (cli_token, device_id) = kimi_store(&cli_home);
    let api_key = resolve_env("KIMI_CODE_API_KEY");
    let auth_token = resolve_env("KIMI_AUTH_TOKEN");
    let (api_value, api_failure) = match api_key {
        Ok(found) => (found, None),
        Err(failure) => (None, Some(failure)),
    };
    let (auth_value, auth_failure) = match auth_token {
        Ok(found) => (found, None),
        Err(failure) => (None, Some(failure)),
    };
    let base_url = env::var("KIMI_CODE_BASE_URL").map_or_else(
        |_| "https://api.kimi.com".into(),
        |value| validated_base(&value, "https://api.kimi.com"),
    );
    kimi_from_resolved(
        api_value,
        api_failure,
        cli_token.as_deref(),
        auth_value,
        auth_failure,
        base_url,
        device_id,
    )
}

pub(crate) fn kimi_from_resolved(
    api_value: Option<(String, String)>,
    api_failure: Option<String>,
    cli_token: Option<&str>,
    auth_value: Option<(String, String)>,
    auth_failure: Option<String>,
    base_url: String,
    device_id: Option<String>,
) -> Provider {
    // Strategy order is API key, then CLI token, then web auth token.
    let mut credentials = Vec::new();
    if let Some((_, value)) = api_value.clone() {
        credentials.push(("api".into(), value));
    }
    if let Some(token) = cli_token {
        credentials.push(("cli".into(), token.into()));
    }
    if let Some((_, value)) = auth_value.clone() {
        credentials.push(("web".into(), value));
    }
    let mut provider = Provider::new(
        "kimi",
        "Kimi Code",
        vec!["KIMI_CODE_API_KEY", "cli", "KIMI_AUTH_TOKEN"],
        credentials,
        AuthStatus::default(),
    );
    provider.configured = api_value.is_some() || cli_token.is_some() || auth_value.is_some();
    provider.auth_status = if !provider.configured {
        match (api_failure, auth_failure) {
            (Some(failure), _) | (None, Some(failure)) => auth_error(&failure),
            (None, None) => missing("No Kimi Code CLI credentials found"),
        }
    } else if cli_token.is_some() {
        available("Kimi Code CLI is signed in", "cli".into())
    } else if let Some((source, _)) = api_value {
        available("API key from KIMI_CODE_API_KEY", source)
    } else {
        let source = auth_value.map_or_else(|| "env".to_owned(), |(source, _)| source);
        available("Web auth token from KIMI_AUTH_TOKEN", source)
    };
    provider.base_url = Some(base_url);
    provider.device_id = device_id.filter(|value| !value.is_empty());
    provider
}
