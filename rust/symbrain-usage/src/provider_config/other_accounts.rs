// Native provider other accounts compatibility implementation.
fn moonshot() -> Provider {
    let resolved = resolve_env("MOONSHOT_API_KEY");
    let (value, error) = match resolved {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    let region = if env::var("MOONSHOT_REGION").ok().as_deref() == Some("cn") {
        "cn"
    } else {
        "ai"
    };
    moonshot_from_resolved(value, error, region)
}

pub(crate) fn moonshot_from_resolved(
    value: Option<(String, String)>,
    error: Option<String>,
    region: &str,
) -> Provider {
    let mut provider = Provider::new(
        "moonshot",
        "Moonshot",
        vec!["MOONSHOT_API_KEY"],
        value.clone().into_iter().collect(),
        AuthStatus::default(),
    );
    provider.configured = value.is_some();
    provider.auth_status = if let Some(error) = error {
        auth_error(&error)
    } else if let Some((source, _)) = value {
        available("API key from MOONSHOT_API_KEY", source)
    } else {
        missing("no API key configured (MOONSHOT_API_KEY)")
    };
    provider.region = if region == "cn" { "cn" } else { "ai" }.into();
    provider
}

fn nous() -> Provider {
    let path = nous_auth_path();
    let resolved = resolve_env_or_file("NOUS_PORTAL_ACCESS_TOKEN", || nous_file_token(&path));
    let (value, error) = match resolved {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    let base_url = env::var("HERMES_PORTAL_BASE_URL").map_or_else(
        |_| "https://portal.nousresearch.com".into(),
        |value| validated_base(&value, "https://portal.nousresearch.com"),
    );
    nous_from_resolved(value, error, base_url)
}

pub(crate) fn nous_from_resolved(
    value: Option<(String, String)>,
    error: Option<String>,
    base_url: String,
) -> Provider {
    let mut provider = Provider::new(
        "nous",
        "Nous Portal",
        vec!["NOUS_PORTAL_ACCESS_TOKEN", "auth.json"],
        value.clone().into_iter().collect(),
        AuthStatus::default(),
    );
    provider.configured = value.is_some();
    provider.auth_status = if let Some((source, _)) = value {
        available(
            "Signed in via Hermes CLI auth store (NOUS_PORTAL_ACCESS_TOKEN or file)",
            source,
        )
    } else if let Some(error) = error {
        auth_error(&error)
    } else {
        missing("No Nous Portal credentials found — sign in with the Hermes CLI")
    };
    provider.base_url = Some(base_url);
    provider
}

fn opencode() -> Provider {
    let resolved = resolve_env("OPENCODE_COOKIE");
    let (value, error) = match resolved {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    let workspace = env_raw("OPENCODE_WORKSPACE_ID");
    opencode_from_resolved(value, error, workspace.as_deref())
}

pub(crate) fn opencode_from_resolved(
    value: Option<(String, String)>,
    error: Option<String>,
    workspace: Option<&str>,
) -> Provider {
    let mut credentials: Vec<(String, String)> = value.clone().into_iter().collect();
    if let Some(workspace) = workspace {
        credentials.push(("workspace".into(), workspace.into()));
    }
    let mut provider = Provider::new(
        "opencode",
        "OpenCode Go",
        vec!["OPENCODE_COOKIE", "OPENCODE_WORKSPACE_ID"],
        credentials,
        AuthStatus::default(),
    );
    provider.configured = value.is_some() || workspace.is_some();
    provider.auth_status = if let Some(error) = error {
        auth_error(&error)
    } else if let Some((source, _)) = value {
        available("Cookie configured (OPENCODE_COOKIE)", source)
    } else if workspace.is_some() {
        available("Workspace override set", "env".into())
    } else {
        missing("No OpenCode Go credentials found — set OPENCODE_COOKIE or run OpenCode Go")
    };
    provider
}

fn openrouter() -> Provider {
    let resolved = resolve_env("OPENROUTER_API_KEY");
    let (value, error) = match resolved {
        Ok(found) => (found, None),
        Err(error) => (None, Some(error)),
    };
    let base_url = env::var("OPENROUTER_API_URL").map_or_else(
        |_| "https://openrouter.ai/api/v1".into(),
        |value| validated_base(&value, "https://openrouter.ai/api/v1"),
    );
    openrouter_from_resolved(value, error, base_url)
}

pub(crate) fn openrouter_from_resolved(
    value: Option<(String, String)>,
    error: Option<String>,
    base_url: String,
) -> Provider {
    let mut provider = Provider::new(
        "openrouter",
        "OpenRouter",
        vec!["OPENROUTER_API_KEY"],
        value.clone().into_iter().collect(),
        AuthStatus::default(),
    );
    provider.configured = value.is_some();
    provider.auth_status = if let Some(error) = error {
        auth_error(&error)
    } else if let Some((source, _)) = value {
        available("API key from OPENROUTER_API_KEY", source)
    } else {
        missing("no API key configured (OPENROUTER_API_KEY)")
    };
    provider.base_url = Some(base_url);
    provider
}

pub(super) fn antigravity_from_process_list(process_list: Option<&str>) -> Provider {
    let running = process_list.is_some_and(antigravity_process_list_running);
    let mut provider = Provider::new(
        "antigravity",
        "Antigravity",
        vec!["local process probe"],
        Vec::new(),
        AuthStatus::default(),
    );
    provider.configured = true;
    provider.auth_status = if running {
        available("Antigravity is running", "local".into())
    } else {
        missing("Antigravity is not running — start the Antigravity app or agy CLI")
    };
    provider
}

fn antigravity() -> Provider {
    let cancellation = crate::Cancellation::with_timeout(Duration::from_secs(2));
    let process_list = super::provider_requests::command_output_with_cancel(
        &cancellation,
        "ps",
        &["-ax", "-o", "pid=,command="],
    );
    antigravity_from_process_list(process_list.as_deref())
}
