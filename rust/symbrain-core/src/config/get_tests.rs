use super::*;
use std::ffi::OsString;
use std::fs;
use tempfile::tempdir;

#[test]
fn get_missing_file_prints_note_to_stdout_and_exits_zero() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let code = run_config_get_with_path(&path, &[], &mut stdout, &mut stderr);
    assert_eq!(code, crate::exit::OK);
    assert_eq!(
        String::from_utf8(stdout).unwrap(),
        format!("(no config file at {})\n", path.display())
    );
    assert!(stderr.is_empty());
}

#[test]
fn get_whole_file_preserves_comments_and_exact_bytes() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let raw = b"# Header comment\n\ndefault_profile = \"personal\"\n\n[audit]\n# inline\nenabled = true\n";
    fs::write(&path, raw).unwrap();

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_config_get_with_path(&path, &[], &mut stdout, &mut stderr);
    assert_eq!(code, crate::exit::OK);
    assert_eq!(stdout, raw);
    assert!(stderr.is_empty());
}

#[test]
fn get_whole_file_succeeds_even_if_toml_malformed() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let raw = b"invalid toml = [ unterminated";
    fs::write(&path, raw).unwrap();

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_config_get_with_path(&path, &[], &mut stdout, &mut stderr);
    assert_eq!(code, crate::exit::OK);
    assert_eq!(stdout, raw);
    assert!(stderr.is_empty());
}

#[test]
fn get_key_from_missing_file_exits_usage_with_not_set_message() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let code = run_config_get_with_path(
        &path,
        &[OsString::from("audit.enabled")],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, crate::exit::USAGE);
    assert!(stdout.is_empty());
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        format!(
            "symbrain config get: key \"audit.enabled\" is not set in {}\n",
            path.display()
        )
    );
}

#[test]
fn get_extra_arguments_rejected() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let code = run_config_get_with_path(
        &path,
        &[OsString::from("key1"), OsString::from("extra_arg")],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, crate::exit::USAGE);
    assert!(stdout.is_empty());
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        "symbrain config get: unexpected argument \"extra_arg\"\n"
    );
}

#[test]
fn get_key_formats_primitives_lists_and_tables_identically_to_go() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let content = r#"
default_profile = "personal"

[audit]
enabled = true
verbose = false
count = 42
threshold = 3.14
items = ["a", "b", "c"]
numbers = [1, 2, 3]

[servers.vault]
binary_path = "/opt/bin/symvault"

[empty]
list = []
table = {}

[nested_inline]
sub = { foo = "bar", num = 10 }
"#;
    fs::write(&path, content).unwrap();

    let test_cases = [
        ("default_profile", "personal\n"),
        ("audit.enabled", "true\n"),
        ("audit.verbose", "false\n"),
        ("audit.count", "42\n"),
        ("audit.threshold", "3.14\n"),
        ("audit.items", "[a b c]\n"),
        ("audit.numbers", "[1 2 3]\n"),
        ("servers.vault.binary_path", "/opt/bin/symvault\n"),
        ("servers.vault", "map[binary_path:/opt/bin/symvault]\n"),
        ("empty.list", "[]\n"),
        ("empty.table", "map[]\n"),
        ("nested_inline.sub", "map[foo:bar num:10]\n"),
        ("nested_inline.sub.foo", "bar\n"),
        (
            "audit",
            "map[count:42 enabled:true items:[a b c] numbers:[1 2 3] threshold:3.14 verbose:false]\n",
        ),
    ];

    for (key, want) in test_cases {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code =
            run_config_get_with_path(&path, &[OsString::from(key)], &mut stdout, &mut stderr);
        assert_eq!(code, crate::exit::OK, "failed for key {key}");
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            want,
            "mismatch for key {key}"
        );
        assert!(stderr.is_empty(), "stderr not empty for key {key}");
    }
}

#[test]
fn get_key_missing_in_file_exits_usage() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "default_profile = \"personal\"\n").unwrap();

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_config_get_with_path(
        &path,
        &[OsString::from("nonexistent.key")],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, crate::exit::USAGE);
    assert!(stdout.is_empty());
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        format!(
            "symbrain config get: key \"nonexistent.key\" is not set in {}\n",
            path.display()
        )
    );
}

#[test]
fn get_key_malformed_toml_exits_generic_with_parse_error() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, "this is invalid toml").unwrap();

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_config_get_with_path(
        &path,
        &[OsString::from("any.key")],
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, crate::exit::GENERIC);
    assert!(stdout.is_empty());
    let err_msg = String::from_utf8(stderr).unwrap();
    assert!(err_msg.starts_with(&format!("symbrain config get: parse {}: ", path.display())));
}
