#![deny(unsafe_code)]

use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use symbrowse_daemon::{SessionRegistry, SessionRegistryOptions};

fn owned_root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "br-registry-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn constructor_defaults_without_creating_profiles() {
    let registry = SessionRegistry::new(SessionRegistryOptions {
        user_data_root: PathBuf::new(),
        ..Default::default()
    });
    assert!(!registry.user_data_root().as_os_str().is_empty());
    assert!(registry.list().is_empty());
}

#[test]
fn registry_normalizes_explicit_root_lexically() {
    let root = owned_root();
    let registry = SessionRegistry::new(SessionRegistryOptions {
        user_data_root: root.join("unused").join("..").join("."),
        ..Default::default()
    });
    assert_eq!(registry.user_data_root(), root);
    assert!(!root.exists(), "construction must not touch the filesystem");
}

#[test]
fn isolation_errors_order_and_clear_preserve_private_profiles() {
    let root = owned_root();
    let registry = SessionRegistry::new(SessionRegistryOptions {
        user_data_root: root.clone(),
        pid: 4242,
        scope: "worktree".into(),
        origin_path: "private-origin".into(),
    });
    for invalid in [
        "",
        "../escape",
        "a/b",
        "bad session",
        "_first",
        "é",
        &"x".repeat(65),
    ] {
        assert!(registry.ensure(invalid).is_err(), "accepted {invalid:?}");
    }
    assert!(!root.exists(), "invalid names must not create profiles");
    let beta = registry.ensure("beta").unwrap();
    let alpha = registry.ensure("alpha").unwrap();
    assert_eq!(
        registry.ensure("alpha").unwrap().started_at,
        alpha.started_at
    );
    assert_ne!(alpha.user_data_dir, beta.user_data_dir);
    assert_ne!(alpha.browser_context_id, beta.browser_context_id);
    assert_eq!(alpha.pid, 4242);
    assert_eq!(alpha.scope, "worktree");
    assert_eq!(alpha.origin_path, "private-origin");
    assert_eq!(
        registry
            .list()
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "beta"]
    );
    assert_eq!(
        registry.get("missing").unwrap_err().to_string(),
        "session not found: \"missing\""
    );
    registry.set_ref("alpha", "save", "@e1").unwrap();
    registry.set_ref("alpha", "save", "@e2").unwrap();
    let mut copied = registry.ref_table("alpha").unwrap();
    copied.insert("injected".into(), "@outside".into());
    assert!(registry.reference("alpha", "injected").is_err());
    assert_eq!(
        registry
            .set_active_tabs("alpha", -1)
            .unwrap_err()
            .to_string(),
        "active tab count cannot be negative"
    );
    registry.set_active_tabs("alpha", 2).unwrap();
    assert_eq!(registry.get("alpha").unwrap().active_tabs, 2);
    assert_eq!(registry.get("alpha").unwrap().ref_count, 1);
    assert_eq!(registry.reference("alpha", "save").unwrap(), "@e2");
    assert!(registry.reference("beta", "save").is_err());
    registry.touch("alpha").unwrap();
    assert!(registry.get("alpha").unwrap().last_activity >= alpha.last_activity);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(root.join("alpha"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    registry.clear();
    assert!(registry.list().is_empty());
    assert!(root.join("alpha").is_dir());
    let restarted = SessionRegistry::new(SessionRegistryOptions {
        user_data_root: root.clone(),
        ..Default::default()
    });
    assert!(
        restarted.list().is_empty(),
        "profile folders must not recreate stale memory/ref state"
    );
    assert_eq!(restarted.ensure("alpha").unwrap().ref_count, 0);
    fs::remove_dir_all(root).unwrap();
}
