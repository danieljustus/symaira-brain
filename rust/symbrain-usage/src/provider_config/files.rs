// Native provider files compatibility implementation.
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

fn json_value(path: &Path) -> Option<Value> {
    serde_json::from_slice(&read_limited(path)?).ok()
}

fn json_string(path: &Path, pointers: &[&str]) -> Option<String> {
    json_value(path)?
        .pointer(&format!("/{}", pointers.join("/")))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(Into::into)
}

fn home() -> PathBuf {
    env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

fn env_path(name: &str, fallback: PathBuf) -> PathBuf {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map_or(fallback, PathBuf::from)
}

// ---------------------------------------------------------------------------
// Provider credential files
// ---------------------------------------------------------------------------

fn codex_home() -> PathBuf {
    env_path("CODEX_HOME", home().join(".codex"))
}

/// `$CODEX_HOME/auth.json`: a top-level `access_token` or the nested
/// `tokens.access_token` the newer CLI writes.
fn codex_file_token(home_dir: &Path) -> Option<String> {
    let contents = read_provider_credentials(&home_dir.join("auth.json"))?;
    // Go decodes this file to map[string]any: exact keys, last duplicate wins,
    // and an overflowing number anywhere invalidates the entire document.
    let root: Value = serde_json::from_str(&go_json_compatible_text(&contents)).ok()?;
    root.get("access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .or_else(|| {
            root.get("tokens")?
                .get("access_token")
                .and_then(Value::as_str)
                .filter(|token| !token.is_empty())
        })
        .map(Into::into)
}

fn copilot_config_dir() -> PathBuf {
    copilot_config_dir_for(&home())
}

fn copilot_config_dir_for(home: &Path) -> PathBuf {
    home.join(".config/github-copilot")
}

/// Best-effort provider parser for `apps.json` then `hosts.json`; the CLI's
/// native route separately requires a strict Go-equivalent candidate below.
fn copilot_file_token() -> Option<String> {
    copilot_file_token_in(&copilot_config_dir())
}

fn copilot_file_token_in(dir: &Path) -> Option<String> {
    for name in ["apps.json", "hosts.json"] {
        let Some(root) = json_value(&dir.join(name)) else {
            continue;
        };
        for prefix in ["github.com:", ""] {
            let Some(entries) = root.as_object() else {
                break;
            };
            if let Some(token) = entries.iter().find_map(|(key, entry)| {
                (prefix.is_empty() || key.starts_with(prefix))
                    .then(|| {
                        entry
                            .get("oauth_token")
                            .and_then(Value::as_str)
                            .filter(|token| !token.is_empty())
                    })
                    .flatten()
            }) {
                return Some(token.to_owned());
            }
        }
    }
    None
}

/// `Ok(None)` means both files are absent. `Err(())` means an existing source
/// cannot be proven to have the same token selection as Go and must be routed
/// to the Go implementation before constructing a provider.
fn copilot_file_token_candidate_in(dir: &Path) -> Result<Option<String>, ()> {
    let apps_path = dir.join("apps.json");
    if let Some(contents) = read_optional_credential_file(&apps_path)? {
        // Go checks apps.json before hosts.json and returns its first usable
        // token. An empty or unknown apps file can affect whether hosts.json
        // is reached, so leave every such case to Go.
        return parse_single_copilot_token(&contents).map(Some);
    }

    let hosts_path = dir.join("hosts.json");
    let Some(contents) = read_optional_credential_file(&hosts_path)? else {
        return Ok(None);
    };
    parse_single_copilot_token(&contents).map(Some)
}

fn read_optional_credential_file(path: &Path) -> Result<Option<Vec<u8>>, ()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    if !metadata.file_type().is_file() {
        return Err(());
    }
    read_limited(path).map(Some).ok_or(())
}

fn parse_single_copilot_token(contents: &[u8]) -> Result<String, ()> {
    let entries: std::collections::BTreeMap<String, CopilotTokenEntry> =
        serde_json::from_slice(contents).map_err(|_| ())?;
    if entries.len() != 1 {
        return Err(());
    }
    entries
        .into_values()
        .next()
        .and_then(|entry| entry.oauth_token)
        .filter(|token| !token.is_empty())
        .ok_or(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CopilotTokenEntry {
    #[serde(rename = "oauth_token")]
    oauth_token: Option<String>,
    #[serde(rename = "user")]
    _user: Option<String>,
}

#[cfg(windows)]
fn usage_home_mismatch_requires_go() -> bool {
    let rust_home = home();
    let go_home = env::var_os("USERPROFILE").map_or_else(|| PathBuf::from("."), PathBuf::from);
    rust_home != go_home
}

#[cfg(not(windows))]
fn usage_home_mismatch_requires_go() -> bool {
    false
}

fn path_may_exist(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(error) => error.kind() != std::io::ErrorKind::NotFound,
    }
}
