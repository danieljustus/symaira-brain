#![deny(unsafe_code)]

#[allow(dead_code)]
#[path = "../src/guard_grants.rs"]
mod guard_grants;

#[test]
fn native_grants_module_compiles_and_handles_empty_store() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut stdout = Vec::new();
    let code = guard_grants::run_at_dir(&["list".into()], dir.path(), &mut stdout);
    assert_eq!(code, 0);
    assert_eq!(stdout, b"No active grants.\n");
}

#[test]
fn native_grants_matches_order_and_revoke_shape() {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        root.path().join("grants.json"),
        br#"[
  {"id":"gnt-old","scope":"device","origin":{"epoch":1722924000,"via":"approval"},"granted_at":"2026-08-06T10:00:00Z","subject":"agent-old","capability":"read_private","purpose":"oracle-purpose","resource":"fs/read_file","scope_ceiling":["session"],"expires_at":"2027-01-01T00:00:00Z"},
  {"id":"gnt-new","scope":"vault","origin":{"epoch":1722924000,"via":"approval"},"granted_at":"2026-08-06T11:00:00Z","subject":"agent-new","capability":"read_private","purpose":"oracle-purpose","resource":"fs/read_file","scope_ceiling":["session"],"expires_at":"2027-01-01T00:00:00Z"}
]"#,
    )
    .expect("seed");
    let mut stdout = Vec::new();
    assert_eq!(
        guard_grants::run_at_dir(&["list".into()], root.path(), &mut stdout),
        0
    );
    assert_eq!(
        stdout,
        b"ID       SCOPE   SUBJECT    ORIGIN               GRANTED_AT\n\
gnt-new  vault   agent-new  approval@1722924000  2026-08-06T11:00:00Z\n\
gnt-old  device  agent-old  approval@1722924000  2026-08-06T10:00:00Z\n"
    );
    stdout.clear();
    assert_eq!(
        guard_grants::run_at_dir(&["revoke".into(), "--all".into()], root.path(), &mut stdout),
        0
    );
    assert_eq!(stdout, b"Revoked 2 grant(s).\n");
    let persisted = std::fs::read_to_string(root.path().join("grants.json")).expect("read");
    assert!(persisted.contains("\"revoked\": true"));
}

#[test]
fn native_grants_keeps_legacy_entries_revivable() {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        root.path().join("grants.json"),
        br#"[{"id":"legacy","scope":"device","subject":"agent-legacy","granted_at":"2026-08-06T10:00:00Z"}]"#,
    )
    .expect("seed");
    let mut stdout = Vec::new();
    assert_eq!(
        guard_grants::run_at_dir(&["list".into()], root.path(), &mut stdout),
        0
    );
    assert_eq!(
        stdout,
        b"ID      SCOPE   SUBJECT       ORIGIN  GRANTED_AT\nlegacy  device  agent-legacy  @0      2026-08-06T10:00:00Z\n"
    );
    stdout.clear();
    assert_eq!(
        guard_grants::run_at_dir(
            &["revoke".into(), "legacy".into()],
            root.path(),
            &mut stdout
        ),
        0
    );
    assert_eq!(stdout, b"Revoked grant legacy.\n");
    assert_eq!(
        std::fs::read_to_string(root.path().join("grants.json")).expect("read"),
        "[\n  {\n    \"id\": \"legacy\",\n    \"scope\": \"device\",\n    \"origin\": {\n      \"epoch\": 0\n    },\n    \"granted_at\": \"2026-08-06T10:00:00Z\",\n    \"subject\": \"agent-legacy\",\n    \"expires_at\": \"0001-01-01T00:00:00Z\",\n    \"revoked\": true\n  }\n]"
    );
}

#[test]
fn native_grants_keeps_go_malformed_diagnostic_for_oracle_case() {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::write(root.path().join("grants.json"), b"{not json\n").expect("seed");
    let mut stdout = Vec::new();
    assert_eq!(
        guard_grants::run_at_dir(&["list".into()], root.path(), &mut stdout),
        0
    );
    let output = String::from_utf8(stdout).expect("utf8");
    assert!(output.contains("invalid character 'n' looking for beginning of object key string"));
}

#[test]
fn native_grants_formats_list_time_and_preserves_persisted_time_shape() {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        root.path().join("grants.json"),
        br#"[{"id":"offset","scope":"device","origin":{"epoch":1},"granted_at":"2026-08-06T12:00:00.123456789+02:00","subject":"agent","expires_at":"2027-01-01T01:00:00+01:00"}]"#,
    )
    .expect("seed");
    let mut stdout = Vec::new();
    guard_grants::run_at_dir(&["list".into()], root.path(), &mut stdout);
    assert!(
        String::from_utf8(stdout)
            .expect("utf8")
            .contains("2026-08-06T10:00:00Z")
    );
    stdout = Vec::new();
    guard_grants::run_at_dir(
        &["revoke".into(), "offset".into()],
        root.path(),
        &mut stdout,
    );
    let persisted = std::fs::read_to_string(root.path().join("grants.json")).expect("read");
    assert!(persisted.contains("2026-08-06T12:00:00.123456789+02:00"));
    assert!(persisted.contains("2027-01-01T01:00:00+01:00"));
}

#[test]
fn native_grants_persist_uses_go_html_escaping() {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        root.path().join("grants.json"),
        "[{\"id\":\"g<&>\",\"scope\":\"device\",\"subject\":\"agent\",\"capability\":\"line\\u2028break\"}]",
    )
    .expect("seed");
    let mut stdout = Vec::new();
    guard_grants::run_at_dir(&["revoke".into(), "g<&>".into()], root.path(), &mut stdout);
    let persisted = std::fs::read_to_string(root.path().join("grants.json")).expect("read");
    assert!(persisted.contains("g\\u003c\\u0026\\u003e"));
    assert!(persisted.contains("line\\u2028break"));
}

#[test]
fn native_grants_does_not_rewrite_ephemeral_revoke() {
    let root = tempfile::tempdir().expect("tempdir");
    let original = br#"[{"id":"run-id","scope":"run","origin":{"epoch":1},"granted_at":"2026-08-06T10:00:00Z","subject":"agent"}]"#;
    std::fs::write(root.path().join("grants.json"), original).expect("seed");
    let mut stdout = Vec::new();
    guard_grants::run_at_dir(
        &["revoke".into(), "run-id".into()],
        root.path(),
        &mut stdout,
    );
    assert_eq!(
        std::fs::read(root.path().join("grants.json")).expect("read"),
        original
    );

    let all_root = tempfile::tempdir().expect("tempdir");
    std::fs::write(all_root.path().join("grants.json"), original).expect("seed");
    stdout.clear();
    guard_grants::run_at_dir(
        &["revoke".into(), "--all".into()],
        all_root.path(),
        &mut stdout,
    );
    assert_eq!(
        std::fs::read(all_root.path().join("grants.json")).expect("read"),
        original
    );
}

#[cfg(unix)]
#[test]
fn native_grants_preserves_existing_store_directory_mode() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let root = tempfile::tempdir().expect("tempdir");
    let store = root.path().join("store");
    std::fs::create_dir(&store).expect("store");
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o750)).expect("mode");
    let mut stdout = Vec::new();
    guard_grants::run_at_dir(&["list".into()], &store, &mut stdout);
    assert_eq!(
        std::fs::metadata(store).expect("metadata").mode() & 0o777,
        0o750
    );
}

#[test]
fn native_grants_opens_store_before_empty_id_usage() {
    let root = tempfile::tempdir().expect("tempdir");
    let store = root.path().join("missing");
    let mut stdout = Vec::new();
    guard_grants::run_at_dir(&["revoke".into(), "".into()], &store, &mut stdout);
    assert!(
        String::from_utf8(stdout)
            .expect("utf8")
            .contains("grants revoke: missing grant ID or --all")
    );
    assert!(store.is_dir());
}

#[test]
fn native_grants_accepts_go_null_defaults_but_rejects_empty_timestamps() {
    let root = tempfile::tempdir().expect("tempdir");
    let grants = root.path().join("grants.json");
    std::fs::write(&grants, b"null").expect("seed null list");
    let mut stdout = Vec::new();
    guard_grants::run_at_dir(&["list".into()], root.path(), &mut stdout);
    assert_eq!(stdout, b"No active grants.\n");

    std::fs::write(
        &grants,
        br#"[{"id":null,"scope":"device","origin":null,"subject":null,"scope_ceiling":[null]}]"#,
    )
    .expect("seed nullable grant");
    stdout.clear();
    guard_grants::run_at_dir(&["list".into()], root.path(), &mut stdout);
    assert!(String::from_utf8(stdout).expect("utf8").contains("device"));

    std::fs::write(
        &grants,
        br#"[{"id":"bad","scope":"device","subject":"agent","granted_at":""}]"#,
    )
    .expect("seed empty timestamp");
    stdout = Vec::new();
    guard_grants::run_at_dir(&["list".into()], root.path(), &mut stdout);
    assert!(
        String::from_utf8(stdout)
            .expect("utf8")
            .contains("parsing time \"\"")
    );
}

#[cfg(unix)]
#[test]
fn native_grants_creates_new_store_directory_with_go_mode() {
    use std::os::unix::fs::MetadataExt;

    let root = tempfile::tempdir().expect("tempdir");
    let store = root.path().join("missing");
    let mut stdout = Vec::new();
    guard_grants::run_at_dir(&["list".into()], &store, &mut stdout);
    assert_eq!(
        std::fs::metadata(store).expect("metadata").mode() & 0o777,
        0o700
    );
}
