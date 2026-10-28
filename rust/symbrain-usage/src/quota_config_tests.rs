use super::*;
#[test]
fn quota_file_credentials_are_bounded_and_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("credentials.toml");
    for content in [
        "",
        "windsurf_api_key = [",
        "windsurf_api_key = 4",
        "windsurf_api_key = ''",
        "windsurf_api_key = 'a'\nwindsurf_api_key = 'b'",
        "windsurf_api_key = \"a\\nb\"",
    ] {
        fs::write(&path, content).unwrap();
        assert!(devin_file_key(&path).is_none());
    }
    fs::write(&path, vec![b'x'; 65_537]).unwrap();
    assert!(devin_file_key(&path).is_none());
    fs::write(
        &path,
        "windsurf_api_key = 'hermetic-key'\napi_server_url = 'https://untrusted.example'\n",
    )
    .unwrap();
    assert_eq!(devin_file_key(&path).as_deref(), Some("hermetic-key"));
    #[cfg(unix)]
    {
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(devin_file_key(&link).is_none());
    }
    assert!(devin_file_key(root.path()).is_none());
}
#[test]
fn quota_resolution_errors_do_not_contain_credentials() {
    for (id, name, var) in [
        ("zai", "Z.ai", "ZAI_API_KEY"),
        ("devin", "Devin", "DEVIN_API_KEY"),
    ] {
        let p = quota_from_resolved(id, name, var, Err("hermetic-secret".into()));
        assert!(!p.configured);
        assert!(!p.auth_status.detail.contains("hermetic-secret"));
    }
}
