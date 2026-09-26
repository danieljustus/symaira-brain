#![deny(unsafe_code)]

use std::path::Path;

use symbrowse_daemon::{
    SESSION_SCHEMA_VERSION, SessionError, SessionRegistry, SessionRegistryOptions,
};

#[test]
fn registry_matches_go_lifecycle_and_stable_errors() {
    let root = tempfile::tempdir().expect("temporary registry root");
    let registry = SessionRegistry::new(SessionRegistryOptions {
        user_data_root: root.path().join("sessions"),
        pid: 4242,
        scope: "worktree".into(),
        origin_path: "/workspace/project".into(),
    });

    let invalid = registry.ensure("../escape").unwrap_err();
    assert!(matches!(&invalid, SessionError::InvalidName(name) if name == "../escape"));
    assert_eq!(invalid.to_string(), "invalid session name: \"../escape\"");
    assert!(!registry.user_data_root().exists());

    let beta = registry.ensure("beta").expect("ensure beta");
    let alpha = registry.ensure("alpha").expect("ensure alpha");
    assert_ne!(alpha.user_data_dir, beta.user_data_dir);
    assert!(Path::new(&alpha.user_data_dir).is_dir());
    assert_eq!(alpha.pid, 4242);
    assert_eq!(alpha.scope, "worktree");
    assert_eq!(alpha.origin_path, "/workspace/project");
    time::OffsetDateTime::parse(
        &alpha.started_at,
        &time::format_description::well_known::Rfc3339,
    )
    .expect("started_at is RFC3339");
    time::OffsetDateTime::parse(
        &alpha.last_activity,
        &time::format_description::well_known::Rfc3339,
    )
    .expect("last_activity is RFC3339");
    assert_eq!(
        registry.ensure("alpha").unwrap().started_at,
        alpha.started_at
    );

    let missing_get = registry.get("missing").unwrap_err();
    assert!(matches!(&missing_get, SessionError::NotFound(name) if name == "missing"));
    assert_eq!(missing_get.to_string(), "session not found: \"missing\"");
    let missing_touch = registry.touch("missing").unwrap_err();
    assert!(matches!(&missing_touch, SessionError::NotFound(name) if name == "missing"));
    assert_eq!(missing_touch.to_string(), "session not found: \"missing\"");
    assert_eq!(
        registry
            .list()
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "beta"]
    );
    let list = registry.list_data();
    assert_eq!(list.schema_version, SESSION_SCHEMA_VERSION);
    assert_eq!(list.sessions.len(), 2);

    registry.touch("alpha").expect("touch existing session");
    registry
        .set_active_tabs("alpha", 3)
        .expect("update active tab count");
    assert_eq!(registry.get("alpha").unwrap().active_tabs, 3);

    assert!(matches!(
        registry.set_ref("alpha", "", "value"),
        Err(SessionError::InvalidValue(_))
    ));
    assert!(matches!(
        registry.set_ref("alpha", "key", ""),
        Err(SessionError::InvalidValue(_))
    ));
    assert!(matches!(
        registry.set_ref("missing", "key", "value"),
        Err(SessionError::NotFound(_))
    ));
    registry.set_ref("alpha", "key", "@element-1").unwrap();
    assert_eq!(registry.reference("alpha", "key").unwrap(), "@element-1");
    assert_eq!(registry.get("alpha").unwrap().ref_count, 1);
    assert!(matches!(
        registry.reference("alpha", "absent"),
        Err(SessionError::InvalidValue(_))
    ));
    assert!(matches!(
        registry.reference("missing", "key"),
        Err(SessionError::NotFound(_))
    ));

    registry.clear();
    assert!(registry.list().is_empty());
    assert!(matches!(
        registry.get("alpha"),
        Err(SessionError::NotFound(name)) if name == "alpha"
    ));
    assert!(
        root.path().join("sessions/alpha").is_dir(),
        "Clear drops registry entries without deleting profile directories"
    );
    assert!(root.path().join("sessions/beta").is_dir());
}

#[test]
fn registry_cleans_profile_root_before_joining_session_names() {
    let root = tempfile::tempdir().expect("temporary registry root");
    let unclean_root = root.path().join("unused").join("..");
    let registry = SessionRegistry::new(SessionRegistryOptions {
        user_data_root: unclean_root,
        ..Default::default()
    });

    assert_eq!(registry.user_data_root(), root.path());
    let info = registry.ensure("alpha").expect("ensure alpha");
    assert_eq!(Path::new(&info.user_data_dir), root.path().join("alpha"));
    assert!(Path::new(&info.user_data_dir).is_dir());

    let empty_root = SessionRegistry::new(SessionRegistryOptions {
        user_data_root: Path::new("").to_owned(),
        ..Default::default()
    });
    assert!(!empty_root.user_data_root().as_os_str().is_empty());
}

#[test]
fn registry_name_validation_matches_go_boundaries_without_side_effects() {
    let root = tempfile::tempdir().expect("temporary registry root");
    let profiles = root.path().join("profiles");
    let registry = SessionRegistry::new(SessionRegistryOptions {
        user_data_root: profiles.clone(),
        pid: 99,
        ..Default::default()
    });

    for name in ["", &"x".repeat(65), "../escape", "ümlaut", "has space"] {
        assert!(
            registry.ensure(name).is_err(),
            "accepted invalid name {name:?}"
        );
    }
    assert!(
        !profiles.exists(),
        "invalid names created a session profile root"
    );
    for name in ["a", &"x".repeat(64)] {
        registry
            .ensure(name)
            .unwrap_or_else(|error| panic!("rejected valid boundary name {name:?}: {error}"));
    }
}
