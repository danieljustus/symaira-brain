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
    home.join(".config/github-copilot")
}

fn copilot_file_token() -> Option<String> {
    copilot_file_token_in(&copilot_config_dir())
}

fn copilot_file_token_in(dir: &Path) -> Option<String> {
    copilot_file_token_candidate_in(dir).ok().flatten()
}

// Preserve the existing gate for unsafe/unreadable sources; regular malformed
// JSON supplies no token and falls through to the next file, as Go does.
fn read_optional_credential_file(path: &Path) -> Result<Option<Vec<u8>>, ()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    if !metadata.file_type().is_file() {
        return Err(());
    }
    if metadata.len() > MAX_CREDENTIAL_FILE_BYTES {
        return Ok(None);
    }
    read_provider_credentials(path).map(Some).ok_or(())
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

#[cfg(test)]
fn path_may_exist(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(error) => error.kind() != std::io::ErrorKind::NotFound,
    }
}
