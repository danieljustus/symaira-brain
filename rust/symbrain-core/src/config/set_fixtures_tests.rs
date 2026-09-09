use super::*;
use std::ffi::OsString;
use std::fs;
use tempfile::tempdir;

#[test]
fn mutate_and_flatten_existing_inline_tables() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let content = r"
[server]
inline = { port = 8080, active = true }
";
    fs::write(&path, content).unwrap();

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_config_set_with_path(
        &path,
        &[OsString::from("server.inline.port"), OsString::from("9090")],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, crate::exit::OK);
    let updated = fs::read_to_string(&path).unwrap();
    assert!(
        updated.contains("[server]\n  [server.inline]\n    active = true\n    port = 9090\n"),
        "expected flattened table, got:\n{updated}"
    );

    // Now set a new key inside the inline table
    let code2 = run_config_set_with_path(
        &path,
        &[OsString::from("server.inline.tls"), OsString::from("true")],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code2, crate::exit::OK);
    let updated2 = fs::read_to_string(&path).unwrap();
    assert!(
        updated2.contains("tls = true"),
        "expected tls in table, got:\n{updated2}"
    );
}

#[test]
fn recursively_flatten_nested_inline_tables() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let content = r#"
top = { mid = { leaf = "hello" } }
"#;
    fs::write(&path, content).unwrap();

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_config_set_with_path(
        &path,
        &[OsString::from("top.mid.leaf"), OsString::from("world")],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, crate::exit::OK);
    let updated = fs::read_to_string(&path).unwrap();
    let expected = "[top]\n  [top.mid]\n    leaf = \"world\"\n";
    assert_eq!(updated, expected);
}

#[test]
fn preserve_arrays_datetime_quoted_keys_and_arrays_of_tables() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let content = r#"
items = [1, 2, 3]
dt = 1979-05-27T07:32:00Z
"quoted.key" = "val"

[[servers]]
name = "alpha"

[[servers]]
name = "beta"
"#;
    fs::write(&path, content).unwrap();

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_config_set_with_path(
        &path,
        &[OsString::from("new_key"), OsString::from("123")],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, crate::exit::OK);

    let updated = fs::read_to_string(&path).unwrap();
    assert!(updated.contains("items = [1, 2, 3]"), "arrays preserved");
    assert!(
        updated.contains("dt = 1979-05-27T07:32:00Z"),
        "datetime preserved"
    );
    assert!(
        updated.contains("\"quoted.key\" = \"val\""),
        "quoted keys preserved"
    );
    assert!(
        updated.contains("[[servers]]\n  name = \"alpha\"\n\n[[servers]]\n  name = \"beta\""),
        "arrays of tables preserved"
    );
    assert!(updated.contains("new_key = 123"), "new key set");
}

#[test]
fn float_encoding_conventions_in_existing_tables() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let content = r"
[metrics]
pos_zero = 0.0
neg_zero = -0.0
pos_inf = inf
neg_inf = -inf
nan = nan
large = 1e6
small = 1e-5
normal = 3.14
int_float = 42.0
";
    fs::write(&path, content).unwrap();

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_config_set_with_path(
        &path,
        &[OsString::from("metrics.other"), OsString::from("test")],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, crate::exit::OK);

    let updated = fs::read_to_string(&path).unwrap();
    assert!(updated.contains("pos_zero = 0.0"), "pos_zero: {updated}");
    assert!(updated.contains("neg_zero = -0.0"), "neg_zero: {updated}");
    assert!(updated.contains("pos_inf = inf"), "pos_inf: {updated}");
    assert!(updated.contains("neg_inf = -inf"), "neg_inf: {updated}");
    assert!(updated.contains("nan = nan"), "nan: {updated}");
    assert!(updated.contains("large = 1e+06"), "large: {updated}");
    assert!(updated.contains("small = 1e-05"), "small: {updated}");
    assert!(updated.contains("normal = 3.14"), "normal: {updated}");
    assert!(updated.contains("int_float = 42.0"), "int_float: {updated}");
}

#[cfg(unix)]
#[test]
fn unix_raw_non_utf8_key_and_value_no_collision_with_ufffd() {
    use std::os::unix::ffi::OsStringExt;

    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");

    let raw_ff_key = OsString::from_vec(b"k_\xff".to_vec());
    let raw_80_key = OsString::from_vec(b"k_\x80".to_vec());
    let ufffd_key = OsString::from_vec(b"k_\xef\xbf\xbd".to_vec());
    let raw_val = OsString::from_vec(b"v_\xff_\x80".to_vec());

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    // Set raw 0xff key
    let code = run_config_set_with_path(
        &path,
        &[raw_ff_key.clone(), raw_val.clone()],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, crate::exit::OK);
    assert!(stderr.is_empty());

    // Set raw 0x80 key
    let code = run_config_set_with_path(
        &path,
        &[raw_80_key, OsString::from("v_80")],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, crate::exit::OK);

    // Set U+FFFD key (UTF-8 bytes 0xEF 0xBF 0xBD)
    let code = run_config_set_with_path(
        &path,
        &[ufffd_key, OsString::from("v_ufffd")],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, crate::exit::OK);

    let raw_bytes = fs::read(&path).unwrap();
    // Verify all three distinct keys exist without colliding
    assert!(
        raw_bytes
            .windows(b"\"k_\x80\" = \"v_80\"".len())
            .any(|w| w == b"\"k_\x80\" = \"v_80\"")
    );
    assert!(
        raw_bytes
            .windows(b"\"k_\xef\xbf\xbd\" = \"v_ufffd\"".len())
            .any(|w| w == b"\"k_\xef\xbf\xbd\" = \"v_ufffd\"")
    );
    assert!(
        raw_bytes
            .windows(b"\"k_\xff\" = \"v_\xff_\x80\"".len())
            .any(|w| w == b"\"k_\xff\" = \"v_\xff_\x80\"")
    );
}

#[cfg(unix)]
#[test]
fn unix_raw_non_utf8_dotted_keys() {
    use std::os::unix::ffi::OsStringExt;

    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");

    let raw_dotted = OsString::from_vec(b"section_\xff.sub_\xfe.leaf".to_vec());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let code = run_config_set_with_path(
        &path,
        &[raw_dotted, OsString::from("custom_val")],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, crate::exit::OK);
    let raw_bytes = fs::read(&path).unwrap();
    assert!(
        raw_bytes
            .windows(b"\"section_\xff\"".len())
            .any(|w| w == b"\"section_\xff\"")
    );
    assert!(
        raw_bytes
            .windows(b"\"sub_\xfe\"".len())
            .any(|w| w == b"\"sub_\xfe\"")
    );
    assert!(
        raw_bytes
            .windows(b"leaf = \"custom_val\"".len())
            .any(|w| w == b"leaf = \"custom_val\"")
    );
}

#[test]
fn deeply_nested_mixed_inline_tables() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let content = r#"
root = { str_val = "abc", num = 123, sub = { leaf = "init", inner = { deep = true } } }
"#;
    fs::write(&path, content).unwrap();

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_config_set_with_path(
        &path,
        &[
            OsString::from("root.sub.inner.deep"),
            OsString::from("false"),
        ],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, crate::exit::OK);
    let updated = fs::read_to_string(&path).unwrap();
    assert!(updated.contains("deep = false"));
    assert!(updated.contains("str_val = \"abc\""));
    assert!(updated.contains("num = 123"));
    assert!(updated.contains("leaf = \"init\""));
}

#[test]
fn datetime_preservation_subseconds_and_tz() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let content = r"
t1 = 1979-05-27T00:32:00.999999-07:00
t2 = 1979-05-27T07:32:00.120000Z
";
    fs::write(&path, content).unwrap();

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_config_set_with_path(
        &path,
        &[OsString::from("extra"), OsString::from("42")],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, crate::exit::OK);
    let updated = fs::read_to_string(&path).unwrap();
    assert!(updated.contains("t1 = 1979-05-27T00:32:00.999999-07:00"));
    assert!(updated.contains("t2 = 1979-05-27T07:32:00.12Z"));
    assert!(updated.contains("extra = 42"));
}
