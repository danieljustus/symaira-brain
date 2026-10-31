include!("opencode_database.rs");

// Official API credentials are independent of explicit cookie/web selection.
fn opencode_api_provider() -> Option<Provider> {
    if env_raw("OPENCODE_GO_API_KEY").is_some() {
        return Some(opencode_api_from_resolved(resolve_env(
            "OPENCODE_GO_API_KEY",
        )));
    }
    if env_raw("OPENCODE_COOKIE").is_some() || env_raw("OPENCODE_WORKSPACE_ID").is_some() {
        return None;
    }
    let dir = env_path(
        "OPENCODE_DATA_DIR",
        env_path("XDG_DATA_HOME", home().join(".local/share")).join("opencode"),
    );
    match opencode_auth_json_key(&dir) {
        Ok(None) => None,
        result => Some(opencode_api_from_resolved(
            result
                .map(|key| key.map(|key| ("file".into(), key)))
                .map_err(str::to_owned),
        )),
    }
}
fn valid_opencode_key(key: &str) -> bool {
    !key.trim().is_empty() && !key.trim().chars().any(char::is_control)
}
fn opencode_api_from_resolved(result: Result<Option<(String, String)>, String>) -> Provider {
    let (credentials, status) = match result {
        Ok(Some((source, key))) if valid_opencode_key(&key) => (
            vec![("go_api".into(), key.trim().into())],
            available("OpenCode Go API key configured", source),
        ),
        Err(error) if error.starts_with("OpenCode v2 database credentials unsupported;") => (
            Vec::new(),
            missing(
                "OpenCode v2 database credentials unsupported; set OPENCODE_GO_API_KEY explicitly",
            ),
        ),
        _ => (
            Vec::new(),
            missing("OpenCode Go credential resolution failed"),
        ),
    };
    Provider::new(
        "opencode",
        "OpenCode Go",
        vec!["OPENCODE_GO_API_KEY", "auth.json"],
        credentials,
        status,
    )
}
fn opencode_auth_json_key(dir: &Path) -> Result<Option<String>, &'static str> {
    #[cfg(unix)]
    let root = match opencode_directory(dir) {
        Ok(root) => cap_std::fs::Dir::from_std_file(root),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(OPENCODE_DB_ERROR),
    };
    #[cfg(unix)]
    let entries = root.entries();
    #[cfg(not(unix))]
    let entries = fs::read_dir(dir);
    let entries = match entries {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(OPENCODE_DB_ERROR),
    };
    let mut databases = Vec::new();
    let mut sidecars = Vec::new();
    for (index, entry) in entries.take(257).enumerate() {
        if index == 256 {
            return Err(OPENCODE_DB_ERROR);
        }
        let name = entry.map_err(|_| OPENCODE_DB_ERROR)?.file_name();
        let name = name.to_str().ok_or(OPENCODE_DB_ERROR)?;
        if name.starts_with("opencode")
            && [".db-wal", ".db-shm", ".db-journal"]
                .iter()
                .any(|suffix| name.ends_with(suffix))
        {
            sidecars.push(name.to_owned());
        }
        if name.starts_with("opencode") && name.strip_suffix(".db").is_some() {
            databases.push(name.to_owned());
        }
    }
    if sidecars.iter().any(|sidecar| {
        databases.len() != 1
            || !["-wal", "-shm", "-journal"]
                .iter()
                .any(|suffix| *sidecar == format!("{}{suffix}", databases[0]))
    }) {
        return Err(OPENCODE_DB_ERROR);
    }
    // No default channel/account substitution when stable and preview coexist.
    if databases.len() > 1 {
        return Err(OPENCODE_DB_ERROR);
    }
    if let Some(name) = databases.first() {
        let (key, authoritative) = opencode_database_key(dir, name)?;
        if authoritative {
            return Ok(key);
        }
    }
    #[cfg(unix)]
    let data = {
        let mut options = cap_std::fs::OpenOptions::new();
        options.read(true);
        cap_std::fs::OpenOptionsExt::custom_flags(
            &mut options,
            libc::O_NOFOLLOW | libc::O_NONBLOCK,
        );
        match root.open_with("auth.json", &options) {
            Ok(file) => {
                let metadata = file.metadata().map_err(|_| OPENCODE_DB_ERROR)?;
                if !metadata.is_file() || metadata.len() > MAX_CREDENTIAL_FILE_BYTES {
                    return Err(OPENCODE_DB_ERROR);
                }
                let mut data = Vec::new();
                file.take(MAX_CREDENTIAL_FILE_BYTES + 1)
                    .read_to_end(&mut data)
                    .map_err(|_| OPENCODE_DB_ERROR)?;
                if data.len() as u64 > MAX_CREDENTIAL_FILE_BYTES {
                    return Err(OPENCODE_DB_ERROR);
                }
                Some(data)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err(OPENCODE_DB_ERROR),
        }
    };
    #[cfg(not(unix))]
    let data = {
        let path = dir.join("auth.json");
        if fs::symlink_metadata(&path).is_ok_and(|m| m.len() > MAX_CREDENTIAL_FILE_BYTES) {
            return Err(OPENCODE_DB_ERROR);
        }
        read_optional_credential_file(&path).map_err(|()| OPENCODE_DB_ERROR)?
    };
    let Some(data) = data else { return Ok(None) };
    opencode_auth_json_bytes(&data)
}

fn opencode_auth_json_bytes(data: &[u8]) -> Result<Option<String>, &'static str> {
    let root: serde_json::Value =
        serde_json::from_slice(data).map_err(|_| "OpenCode auth.json invalid")?;
    let root = root.as_object().ok_or("OpenCode auth.json invalid")?;
    let Some(entry) = root.get("opencode-go") else {
        return Ok(None);
    };
    let entry = entry.as_object().ok_or("OpenCode Go credential invalid")?;
    let Some(key) = entry.get("key") else {
        return Ok(None);
    };
    let key = key.as_str().ok_or("OpenCode Go credential invalid")?;
    if key.is_empty() {
        return Ok(None);
    }
    if !valid_opencode_key(key) {
        return Err("OpenCode Go credential invalid");
    }
    Ok(Some(key.trim().into()))
}

#[cfg(test)]
mod opencode_api_discovery_tests {
    use super::*;
    #[test]
    #[cfg(unix)]
    fn opencode_v2_active_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let db = rusqlite::Connection::open(dir.path().join("opencode.db")).unwrap();
        db.execute_batch("CREATE TABLE migration(id TEXT PRIMARY KEY,time_completed INTEGER); CREATE TABLE credential(id TEXT PRIMARY KEY,integration_id TEXT,value TEXT,active INTEGER,time_updated INTEGER); INSERT INTO migration VALUES('20260805200742_import_legacy_credentials',1); INSERT INTO credential VALUES('current','opencode-go','{\"key\":\"synthetic-current\"}',1,1);").unwrap();
        drop(db);
        assert_eq!(
            opencode_auth_json_key(dir.path()).unwrap().as_deref(),
            Some("synthetic-current")
        );
    }

    #[test]
    fn opencode_api_authjson_and_database_authority() {
        let dir = tempfile::tempdir().unwrap();
        let auth = dir.path().join("auth.json");
        fs::write(
            &auth,
            br#"{"unrelated":false,"opencode-go":{"key":"synthetic-key"}}"#,
        )
        .unwrap();
        assert_eq!(
            opencode_auth_json_key(dir.path()).unwrap().as_deref(),
            Some("synthetic-key")
        );
        fs::write(dir.path().join("opencode-next.db"), []).unwrap();
        assert!(
            opencode_auth_json_key(dir.path())
                .unwrap_err()
                .contains("database")
        );
        let provider = opencode_api_from_resolved(Ok(Some(("env".into(), "explicit-key".into()))));
        assert!(provider.configured);
        assert_eq!(provider.credentials[0].0, "go_api");
        assert!(!opencode_api_from_resolved(Err("secret should not escape".into())).configured);
    }
    #[test]
    fn opencode_api_malformed_and_unsafe_files_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        for body in [
            "null",
            "{",
            r#"{"opencode-go":false}"#,
            r#"{"opencode-go":{"key":true}}"#,
        ] {
            fs::write(dir.path().join("auth.json"), body).unwrap();
            assert!(opencode_auth_json_key(dir.path()).is_err());
        }
        fs::remove_file(dir.path().join("auth.json")).unwrap();
        fs::create_dir(dir.path().join("auth.json")).unwrap();
        assert!(opencode_auth_json_key(dir.path()).is_err());
    }
}
