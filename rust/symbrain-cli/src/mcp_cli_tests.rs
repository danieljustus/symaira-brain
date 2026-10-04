use super::*;

#[test]
fn inline_flags_accept_both_dash_forms() {
    assert_eq!(
        inline_value("-profile=one", "profile"),
        Some("one".to_string())
    );
    assert_eq!(
        inline_value("--profile=one", "profile"),
        Some("one".to_string())
    );
    assert_eq!(
        inline_value("-profile-file=room.toml", "profile-file"),
        Some("room.toml".to_string())
    );
}

#[test]
fn resolved_optional_path_is_gated_and_remains_lazy() {
    let owned = tempfile::tempdir().expect("owned directory");
    let path = owned.path().join("owned-worker.exe");
    std::fs::write(&path, b"not a runnable child").expect("owned fixture");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let profile = symbrain_policy::profile::parse::parse(
        "owned",
        "[profile]\nname='owned'\n[servers.operate]\nenabled=true\n",
    )
    .expect("profile");
    let mut config = symbrain_core::config::resolved::BrainConfig::default();
    config.servers.operate = symbrain_core::go_path::os_bytes(path.as_os_str()).into();
    for enabled in [false, true] {
        config.modules.operate = enabled;
        let (mut stderr, mut managed, mut backends) = (Vec::new(), Vec::new(), BTreeMap::new());
        build_backends(
            &profile,
            &config,
            None,
            &mut stderr,
            &mut managed,
            &mut backends,
        );
        assert!(stderr.is_empty());
        assert_eq!(managed.len(), usize::from(enabled));
        assert_eq!(backends.contains_key("operate"), enabled);
        for child in managed {
            assert_eq!(child.state(), symbrain_broker::State::Idle);
            child.shutdown();
        }
    }
}

#[test]
fn url_only_foreign_server_matches_go_warning_and_is_skipped() {
    let profile = symbrain_policy::profile::parse::parse(
        "url-only",
        r#"
[profile]
name = "url-only"

[servers.remote]
enabled = true
url = "https://mcp.example.com"
"#,
    )
    .expect("valid profile");
    let mut stderr = Vec::new();
    let mut managed = Vec::new();
    let mut backends = BTreeMap::new();

    build_backends(
        &profile,
        &symbrain_core::config::resolved::BrainConfig::default(),
        None,
        &mut stderr,
        &mut managed,
        &mut backends,
    );

    assert_eq!(
        String::from_utf8(stderr).expect("UTF-8 warning"),
        "symbrain mcp: remote: url-only foreign server (no stdio transport yet); skipping\n"
    );
    assert!(managed.is_empty());
    assert!(backends.is_empty());
}
