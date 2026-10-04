//! Prepared regression controls for six independently reviewed startup seams.
use crate::{startup_rotation_json as rotation, startup_time::Time};

#[test]
fn time_admission_is_distinct_from_marshal_and_json_unescaping() {
    for (raw, expected) in [
        (br#""2099-01-01T0:00:00Z""#.as_slice(), "2099-01-01T00:00:00Z"),
        (br#""2099-01-01T00:00:00,123456789123Z""#, "2099-01-01T00:00:00.123456789Z"),
        (br#""2099-01-01T00:00:00+00:60""#, "2099-01-01T00:00:00+01:00"),
    ] {
        assert_eq!(Time::json(raw).unwrap().render().unwrap(), expected);
    }
    for raw in [
        br#""2099-01-01t00:00:00Z""#.as_slice(),
        br#""2099-01-01T00:00:00z""#,
        br#""2099-01-01 00:00:00Z""#,
        br#""2099-01-01T00:00:60Z""#,
        br#""\u0032099-01-01T00:00:00Z""#,
    ] { assert!(Time::json(raw).is_err()); }
    let parsed = Time::json(br#""2099-01-01T00:00:00+24:00""#).unwrap();
    assert_eq!(parsed.render().unwrap_err().as_ref(), b"Time.MarshalJSON: timezone hour outside of range [0,23]");
}

#[test]
fn authenticated_json_checks_all_syntax_before_types_but_time_is_fatal() {
    for (raw, expected) in [
        (br#"[{"secret":7}]"#.as_slice(), "json: cannot unmarshal number into Go struct field fallbackEntry.secret of type string"),
        (br#"[{"secret":7,"expires_at":false}]"#, "Time.UnmarshalJSON: input is not a JSON string"),
        (br#"[{"secret":7,"expires_at":false,}]"#, "invalid character '}' looking for beginning of object key string"),
        (br#"[{"unknown":1e+}]"#, "invalid character '}' in exponent of numeric literal"),
    ] {
        let error = match rotation::parse(raw) { Ok(_) => panic!("invalid record admitted"), Err(error) => error };
        assert_eq!(error.as_ref(), expected.as_bytes());
    }
    let records = rotation::parse(br#"[{"secret":"first","Secret":"last","secret":null,"expires_at":null,"unknown":1e400}]"#).unwrap();
    assert_eq!(rotation::render(&records).unwrap(), "[{\"secret\":\"last\",\"expires_at\":\"0001-01-01T00:00:00Z\"}]");
}

#[test]
fn time_error_preserves_bytewise_quote_and_range_precedence() {
    let error = match Time::json(b"\"2099-01-01T00:00:00\xff\"") { Ok(_) => panic!("invalid raw zone admitted"), Err(error) => error };
    assert_eq!(error.as_ref(), b"parsing time \"2099-01-01T00:00:00\\xff\" as \"2006-01-02T15:04:05Z07:00\": cannot parse \"\\xff\" as \"Z07:00\"");
    let error = match Time::json(br#""2099-01-01T00:00:00?25:61""#) { Ok(_) => panic!("invalid zone admitted"), Err(error) => error };
    assert_eq!(error.as_ref(), b"parsing time \"2099-01-01T00:00:00?25:61\": time zone offset minute out of range");
}

#[test]
fn nested_mkdir_names_the_original_blocker_and_preserves_it() {
    use std::{fs, path::PathBuf};
    let mut entropy = [0_u8; 8]; getrandom::fill(&mut entropy).unwrap();
    let root = std::env::temp_dir().join(format!("memory-nested-blocker-{:016x}", u64::from_le_bytes(entropy)));
    fs::create_dir(&root).unwrap();
    #[cfg(unix)]
    let name = symbrain_core::go_path::from_bytes(b"blocked-\xff\xe2\x82");
    #[cfg(not(unix))]
    let name = std::ffi::OsString::from("blocked");
    let blocker = root.join(name); fs::write(&blocker, b"unchanged owned blocker").unwrap();
    let error = crate::startup_mkdir::private(&blocker.join("new/leaf")).unwrap_err();
    let mut expected = b"mkdir ".to_vec(); expected.extend(symbrain_core::go_path::os_bytes(blocker.as_os_str())); expected.extend(b": not a directory");
    assert_eq!(error.as_ref(), expected);
    assert_eq!(fs::read(&blocker).unwrap(), b"unchanged owned blocker");
    assert!(!PathBuf::from(&blocker).join("new").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn incompatible_schema_remains_migration_failure_with_atomic_rollback() {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    connection.execute_batch("CREATE TABLE memories(id TEXT PRIMARY KEY); CREATE TABLE rules(id TEXT PRIMARY KEY); CREATE TABLE schema_migrations(version TEXT PRIMARY KEY);").unwrap();
    let error = match crate::migration::configure_with_phase(connection) {
        Ok(_) => panic!("corrupt legacy schema accepted"), Err(error) => error,
    };
    assert!(matches!(error.phase, crate::migration::ConfigurePhase::Migration));
    assert!(crate::startup_db_error::format(error.phase, &error.error).as_ref().starts_with(b"failed to run migrations: "));
    // The unchanged migration test exercises the public on-disk rollback and
    // verifies both absent repair columns and absent migration markers.
}
