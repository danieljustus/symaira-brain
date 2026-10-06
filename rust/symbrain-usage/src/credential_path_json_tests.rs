//! Lexical credential joins and bounded credential JSON admission.
#[cfg(unix)]
use super::credential_join;
use super::{
    CredentialFields, MAX_CREDENTIAL_FILE_BYTES, go_json_credential_limits,
    read_provider_credentials,
};
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
#[test]
fn credential_join_cleans_like_go_filepath_join() {
    for (base, child, expected) in [
        ("/home/u", ".kimi-code", "/home/u/.kimi-code"),
        (
            "/home/u/",
            "credentials/kimi-code.json",
            "/home/u/credentials/kimi-code.json",
        ),
        ("/a/b", "../c", "/a/c"),
        ("/a/b/c", "../../d/./e", "/a/d/e"),
        ("/", "../x", "/x"),
        ("/a", "..", "/"),
        ("a", "..", "."),
        ("a", "../../c", "../c"),
        ("../a", "../b", "../b"),
        ("a/./b//", ".", "a/b"),
        ("", "x/y", "x/y"),
        ("", "", ""),
        (".", "", "."),
    ] {
        assert_eq!(
            credential_join(Path::new(base), child),
            PathBuf::from(expected),
            "{base:?} + {child:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn credential_join_preserves_non_utf8_unix_bytes() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let base = Path::new(OsStr::from_bytes(b"/home/\xff"));
    let joined = credential_join(base, "x/../device_id");
    assert_eq!(joined.as_os_str().as_bytes(), b"/home/\xff/device_id");
}

#[test]
fn credential_limits_follow_go_depth_and_number_rules() {
    assert!(go_json_credential_limits(
        r#"{"a":[1,{"b":"x\"]}"}]}"#,
        true
    ));
    let nested = format!("{}{}", "[".repeat(10_000), "]".repeat(10_000));
    assert!(go_json_credential_limits(&nested, false));
    let too_deep = format!("{}{}", "[".repeat(10_001), "]".repeat(10_001));
    assert!(!go_json_credential_limits(&too_deep, false));
    // Generic decoding converts numbers to finite float64; typed decoding
    // ignores numbers in unknown metadata.
    assert!(!go_json_credential_limits(r#"{"n":1e400}"#, true));
    assert!(go_json_credential_limits(r#"{"n":1e400}"#, false));
    assert!(go_json_credential_limits(r#"{"n":-2.5E+3}"#, true));
    // Brackets and digits inside strings are not structure or numbers.
    assert!(go_json_credential_limits(r#"{"s":"[[[1e400"}"#, true));
}

#[test]
fn credential_fields_keep_order_duplicates_and_null() {
    let fields: CredentialFields = serde_json::from_str(r#"{"b":1,"a":"x","b":null}"#).unwrap();
    let names: Vec<_> = fields.0.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["b", "a", "b"]);
    assert_eq!(fields.0[0].1.get(), "1");
    assert!(
        serde_json::from_str::<CredentialFields>("null")
            .unwrap()
            .0
            .is_empty()
    );
    let error = serde_json::from_str::<CredentialFields>("[1]")
        .err()
        .expect("arrays are not credential objects")
        .to_string();
    assert!(error.contains("a credential object or null"), "{error}");
}

#[test]
fn credential_reads_are_bounded_regular_files() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("credentials.json");
    std::fs::write(&file, b"{}").unwrap();
    assert_eq!(
        read_provider_credentials(&file).as_deref(),
        Some(&b"{}"[..])
    );
    let limit = usize::try_from(MAX_CREDENTIAL_FILE_BYTES).unwrap();
    std::fs::write(&file, vec![b' '; limit]).unwrap();
    assert_eq!(
        read_provider_credentials(&file).map(|data| data.len()),
        Some(limit)
    );
    std::fs::write(&file, vec![b' '; limit + 1]).unwrap();
    assert_eq!(read_provider_credentials(&file), None);
    assert_eq!(
        read_provider_credentials(&dir.path().join("missing.json")),
        None
    );
    std::fs::create_dir(dir.path().join("directory.json")).unwrap();
    assert_eq!(
        read_provider_credentials(&dir.path().join("directory.json")),
        None
    );
    assert_eq!(read_provider_credentials(Path::new("/")), None);
    #[cfg(unix)]
    {
        let link = dir.path().join("link.json");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert_eq!(
            read_provider_credentials(&link),
            None,
            "symlinks are not followed"
        );
    }
}
