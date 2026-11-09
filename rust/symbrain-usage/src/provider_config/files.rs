// Native provider files compatibility implementation.
fn read_limited(path: &Path) -> Option<Vec<u8>> {
    read_provider_credentials(path)
}

#[cfg(test)]
fn json_value(path: &Path) -> Option<Value> {
    serde_json::from_slice(&read_limited(path)?).ok()
}

#[cfg(test)]
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

fn copilot_config_dir() -> PathBuf {
    copilot_config_dir_for(&home())
}

fn copilot_config_dir_for(home: &Path) -> PathBuf {
    credential_join(home, ".config/github-copilot")
}

fn copilot_file_token() -> Option<String> {
    copilot_file_token_in(&copilot_config_dir())
}

// Preserve the existing gate for unsafe/unreadable sources; regular malformed
// JSON supplies no token and falls through to the next file, as Go does.
