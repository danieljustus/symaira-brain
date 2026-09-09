use super::provider_requests::validated_base;
use super::{AuthStatus, MAX_CREDENTIAL_FILE_BYTES, Provider, Value};
use std::env;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

fn env_credential(name: &str) -> Option<(String, String)> {
    env::var(name).ok().filter(|v| !v.is_empty()).and_then(|v| {
        if let Some(name) = v.strip_prefix("env://") {
            return env::var(name)
                .ok()
                .filter(|value| !value.is_empty())
                .map(|value| ("env".into(), value));
        }
        if let Some(path) = v
            .strip_prefix("symvault://")
            .or_else(|| v.strip_prefix("vault://"))
            .filter(|path| {
                !path.is_empty() && !path.starts_with('-') && !path.chars().any(char::is_control)
            })
        {
            return resolve_secret_command("symvault", &["get", "--", path, "--print"])
                .map(|value| ("vault".into(), value));
        }
        if let Some(reference) = v.strip_prefix("keychain://") {
            let (service, account) = reference.split_once('/')?;
            if service.is_empty() || account.is_empty() {
                return None;
            }
            return resolve_secret_command(
                "security",
                &["find-generic-password", "-w", "-s", service, "-a", account],
            )
            .map(|value| ("keychain".into(), value));
        }
        if is_secret_reference(&v) {
            return None;
        }
        Some(("env".into(), v))
    })
}
fn resolve_secret_command(command: &str, args: &[&str]) -> Option<String> {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let output_file = tempfile::NamedTempFile::new().ok()?;
    let output = output_file.as_file().try_clone().ok()?;
    let mut child_command = std::process::Command::new(command);
    child_command
        .args(args)
        .stdout(Stdio::from(output))
        .stderr(Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut child_command, 0);
    let mut child = child_command.spawn().ok()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(None) | Err(_) => {
                terminate_secret_command(&mut child);
                return None;
            }
        }
    };
    terminate_secret_command(&mut child);
    if !status.success() {
        return None;
    }
    let mut file = output_file.as_file().try_clone().ok()?;
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut output = Vec::new();
    file.take(MAX_CREDENTIAL_FILE_BYTES + 1)
        .read_to_end(&mut output)
        .ok()?;
    let max_bytes = usize::try_from(MAX_CREDENTIAL_FILE_BYTES).ok()?;
    if output.len() > max_bytes {
        return None;
    }
    String::from_utf8(output)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn terminate_secret_command(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let _ = rustix::process::kill_process_group(
            rustix::process::Pid::from_child(child),
            rustix::process::Signal::KILL,
        );
    }
    let _ = child.kill();
    let _ = child.wait();
}
fn is_secret_reference(value: &str) -> bool {
    ["symvault://", "vault://", "env://", "keychain://"]
        .iter()
        .any(|prefix| value.starts_with(prefix))
}
fn read_limited(path: &Path) -> Option<Vec<u8>> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NOFOLLOW);
    let file = options.open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_CREDENTIAL_FILE_BYTES {
        return None;
    }
    let mut contents = Vec::new();
    file.take(MAX_CREDENTIAL_FILE_BYTES + 1)
        .read_to_end(&mut contents)
        .ok()?;
    (contents.len() <= usize::try_from(MAX_CREDENTIAL_FILE_BYTES).ok()?).then_some(contents)
}
fn json_string(path: &Path, pointers: &[&str]) -> Option<String> {
    let v: Value = serde_json::from_slice(&read_limited(path)?).ok()?;
    pointers
        .iter()
        .try_fold(&v, |v, key| v.get(*key))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(Into::into)
}
fn home() -> PathBuf {
    env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}
fn configured(
    id: &str,
    name: &str,
    sources: Vec<&'static str>,
    envs: &[&str],
    file: Option<(PathBuf, Vec<&str>)>,
) -> Provider {
    let mut creds = Vec::new();
    for env_name in envs {
        if let Some((source, v)) = env_credential(env_name) {
            creds.push((source, v));
        }
    }
    let file_credential = file
        .filter(|_| creds.is_empty())
        .and_then(|(path, keys)| json_string(&path, &keys));
    if let Some(value) = file_credential {
        creds.push(("file".into(), value));
    }
    let status = if creds.is_empty() {
        AuthStatus {
            status: "missing".into(),
            detail: format!("No {name} credentials found"),
            source: None,
        }
    } else {
        AuthStatus {
            status: "available".into(),
            detail: format!("Signed in via {name}"),
            source: Some(creds[0].0.clone()),
        }
    };
    Provider::new(id, name, sources, creds, status)
}

#[must_use]
pub fn all_providers() -> Vec<Provider> {
    vec![
        claude(),
        codex(),
        copilot(),
        cursor(),
        kimi(),
        moonshot(),
        nous(),
        opencode(),
        openrouter(),
        antigravity(),
    ]
}

fn claude() -> Provider {
    let mut provider = configured(
        "claude",
        "Claude",
        vec![
            "ANTHROPIC_ADMIN_KEY",
            "ANTHROPIC_OAUTH_TOKEN",
            "file",
            "keychain",
        ],
        &["ANTHROPIC_ADMIN_KEY", "ANTHROPIC_OAUTH_TOKEN"],
        Some((
            home().join(".claude/.credentials.json"),
            vec!["oauthAccount", "default", "accessToken"],
        )),
    );
    if let Some((source, value)) = env_credential("ANTHROPIC_OAUTH_TOKEN") {
        provider
            .credentials
            .retain(|(_, current)| current != &value);
        provider.credentials.push((source, value));
    }
    provider
}

fn codex() -> Provider {
    let mut provider = configured(
        "codex",
        "Codex",
        vec!["CODEX_ACCESS_TOKEN", "auth.json"],
        &["CODEX_ACCESS_TOKEN"],
        None,
    );
    if provider.credentials.is_empty() {
        let path = env::var_os("CODEX_HOME")
            .map_or_else(|| home().join(".codex"), PathBuf::from)
            .join("auth.json");
        if let Some(value) = json_string(&path, &["access_token"]) {
            provider.credentials.push(("file".into(), value.clone()));
            provider.credential = Some(value);
            provider.configured = true;
        }
    }
    provider
}

fn copilot() -> Provider {
    let mut provider = configured(
        "copilot",
        "GitHub Copilot",
        vec!["COPILOT_ACCESS_TOKEN", "apps.json", "hosts.json"],
        &["COPILOT_ACCESS_TOKEN"],
        None,
    );
    if provider.credentials.is_empty() {
        for name in ["apps.json", "hosts.json"] {
            if let Some(value) = json_string(
                &home().join(".config/github-copilot").join(name),
                &["github.com:", "oauth_token"],
            ) {
                provider.credentials.push(("file".into(), value.clone()));
                provider.credential = Some(value);
                provider.configured = true;
                break;
            }
        }
    }
    provider
}

fn cursor() -> Provider {
    configured(
        "cursor",
        "Cursor",
        vec!["CURSOR_COOKIE"],
        &["CURSOR_COOKIE"],
        None,
    )
}

fn kimi() -> Provider {
    let mut provider = configured(
        "kimi",
        "Kimi Code",
        vec!["KIMI_CODE_API_KEY", "cli", "KIMI_AUTH_TOKEN"],
        &["KIMI_CODE_API_KEY", "KIMI_AUTH_TOKEN"],
        None,
    );
    let kimi_home =
        env::var_os("KIMI_CODE_HOME").map_or_else(|| home().join(".kimi-code"), PathBuf::from);
    if let Some(value) = json_string(
        &kimi_home.join("credentials/kimi-code.json"),
        &["access_token"],
    ) {
        let index = provider.credentials.len().min(1);
        provider
            .credentials
            .insert(index, ("cli".into(), value.clone()));
        if provider.credential.is_none() {
            provider.credential = Some(value);
        }
        provider.configured = true;
        provider.auth_status = AuthStatus {
            status: "available".into(),
            detail: "Signed in via Kimi Code CLI".into(),
            source: Some("cli".into()),
        };
    }
    provider
}

fn moonshot() -> Provider {
    let mut provider = configured(
        "moonshot",
        "Moonshot",
        vec!["MOONSHOT_API_KEY"],
        &["MOONSHOT_API_KEY"],
        None,
    );
    provider.region = if env::var("MOONSHOT_REGION").ok().as_deref() == Some("cn") {
        "cn".into()
    } else {
        "ai".into()
    };
    provider
}

fn nous() -> Provider {
    let mut provider = configured(
        "nous",
        "Nous Portal",
        vec!["NOUS_PORTAL_ACCESS_TOKEN", "auth.json"],
        &["NOUS_PORTAL_ACCESS_TOKEN"],
        None,
    );
    if provider.credentials.is_empty() {
        let path = env::var_os("HERMES_HOME")
            .map_or_else(|| home().join(".hermes"), PathBuf::from)
            .join("auth.json");
        if let Some(value) = json_string(&path, &["providers", "0", "access_token"]) {
            provider.credentials.push(("file".into(), value.clone()));
            provider.credential = Some(value);
            provider.configured = true;
        }
    }
    provider
}

fn opencode() -> Provider {
    let mut provider = configured(
        "opencode",
        "OpenCode Go",
        vec!["OPENCODE_COOKIE", "OPENCODE_WORKSPACE_ID"],
        &["OPENCODE_COOKIE"],
        None,
    );
    if let Some(value) = env::var("OPENCODE_WORKSPACE_ID")
        .ok()
        .filter(|value| !value.is_empty())
    {
        provider.credentials.push(("workspace".into(), value));
        provider.configured = true;
    }
    provider
}

fn openrouter() -> Provider {
    let mut provider = configured(
        "openrouter",
        "OpenRouter",
        vec!["OPENROUTER_API_KEY"],
        &["OPENROUTER_API_KEY"],
        None,
    );
    provider.base_url = Some(env::var("OPENROUTER_API_URL").map_or_else(
        |_| "https://openrouter.ai/api/v1".into(),
        |value| validated_base(&value, "https://openrouter.ai/api/v1"),
    ));
    provider
}

fn antigravity() -> Provider {
    Provider::new(
        "antigravity",
        "Antigravity",
        vec!["local process probe"],
        Vec::new(),
        AuthStatus {
            status: "missing".into(),
            detail: "Antigravity is not running — start the Antigravity app or agy CLI".into(),
            source: None,
        },
    )
}

#[cfg(test)]
#[path = "provider_config_tests.rs"]
mod tests;
